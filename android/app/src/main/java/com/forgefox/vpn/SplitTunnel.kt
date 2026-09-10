package com.forgefox.vpn

import android.content.SharedPreferences
import android.net.VpnService
import java.net.InetAddress

/**
 * Site-based split tunneling.
 *
 * The Rust core is a plain L3 bridge (everything that enters the TUN is
 * forwarded to the SSH server), so per-site rules are enforced with routes on
 * the VpnService.Builder:
 *  - Proxy mode:  route ONLY the selected sites into the VPN.
 *  - Bypass mode: route everything EXCEPT the selected sites. Android routes
 *    are additive and there is no "exclude" — so we netsplit 0.0.0.0/0 around
 *    the excluded ranges; destinations not covered fall through to the
 *    physical network.
 *
 * Domain / domain-zone rules get two kinds of input:
 *  - a one-shot resolve at connect time (common cases work immediately), and
 *  - addresses the Rust core learns by watching DNS answers as they cross
 *    the tunnel (CDNs rotate addresses constantly) — handed over through
 *    [ForgeFoxVpnService.onDnsLearned], which re-establishes the TUN with
 *    the new routes.
 */
object SplitTunnel {

    /** Inclusive IPv4 range, uint32 stored in a Long. */
    data class Range(val start: Long, val end: Long)

    /** Site rules (everything except app rules) from prefs. */
    fun siteRules(prefs: SharedPreferences): List<SplitRules.Rule> =
        SplitRules.load(prefs)
            .filter { it.enabled && it.type != SplitRules.Type.APP }

    /**
     * Resolve site rules to IPv4 ranges: literals directly, plus a one-shot
     * seed of domain rules (see [seedDomainRanges]) and every address the
     * Rust core learned from live DNS answers. Re-resolving here is
     * deliberate: this must stay fast enough to run on every TUN rebuild.
     */
    fun resolveRanges(
        rules: List<SplitRules.Rule>,
        learnedIps: Set<Long>,
        seed: List<Range>,
        log: (String) -> Unit
    ): List<Range> {
        val ranges = mutableListOf<Range>()
        for (rule in rules) {
            val entry = rule.normalized
            try {
                when (rule.type) {
                    SplitRules.Type.CIDR -> {
                        val parts = entry.split("/")
                        val prefix = parts[1].toIntOrNull()
                            ?: throw IllegalArgumentException("bad prefix")
                        if (prefix < 0 || prefix > 32) throw IllegalArgumentException("prefix out of range")
                        val base = ipToInt(parts[0]) and maskOf(prefix)
                        ranges.add(Range(base, base + maskInv(prefix)))
                    }
                    SplitRules.Type.IP -> ranges.add(Range(ipToInt(entry), ipToInt(entry)))
                    else -> {} // domains: seeded once per connect + live DNS learning
                }
            } catch (e: Exception) {
                log("Rule '${rule.type.wire}:$entry' skipped: ${e.message}")
            }
        }
        ranges.addAll(seed)
        // Addresses learned from DNS answers after connect.
        for (ip in learnedIps) {
            ranges.add(Range(ip, ip))
        }
        return mergeRanges(ranges)
    }

    /**
     * One-shot parallel resolve of domain rules for route seeding, with a
     * hard time budget. This must never run per-rebuild: DPI-blocked domains
     * hang the system resolver for ~10s each, and a serial loop over ~20
     * rules stalls VPN startup for minutes. Unresolved domains are simply
     * skipped — the DNS observer in the Rust core learns those addresses
     * once the tunnel is up.
     */
    fun seedDomainRanges(
        rules: List<SplitRules.Rule>,
        budgetMs: Long = 4000,
        log: (String) -> Unit
    ): List<Range> {
        val domainRules = rules.filter {
            it.enabled && (it.type == SplitRules.Type.DOMAIN || it.type == SplitRules.Type.DOMAIN_ZONE)
        }
        if (domainRules.isEmpty()) return emptyList()

        val pool = java.util.concurrent.Executors.newFixedThreadPool(minOf(8, domainRules.size))
        try {
            val futures = domainRules.map { rule ->
                pool.submit<MutableList<Range>> {
                    val out = mutableListOf<Range>()
                    try {
                        for (addr in InetAddress.getAllByName(rule.normalized)) {
                            val b = addr.address
                            if (b.size == 4) {
                                val v = bytesToInt(b)
                                out.add(Range(v, v))
                            }
                        }
                    } catch (_: Exception) {}
                    out
                }
            }
            val deadline = System.currentTimeMillis() + budgetMs
            val ranges = mutableListOf<Range>()
            for ((i, f) in futures.withIndex()) {
                val remaining = deadline - System.currentTimeMillis()
                if (remaining <= 0) {
                    f.cancel(true)
                    log("Seed '${domainRules[i].normalized}': timed out (DNS observer will pick it up)")
                    continue
                }
                try {
                    val got = f.get(remaining, java.util.concurrent.TimeUnit.MILLISECONDS)
                    log("Seed '${domainRules[i].normalized}': ${got.size} addresses")
                    ranges.addAll(got)
                } catch (e: Exception) {
                    f.cancel(true)
                    log("Seed '${domainRules[i].normalized}': unresolved (${e.javaClass.simpleName})")
                }
            }
            return mergeRanges(ranges)
        } finally {
            pool.shutdownNow()
        }
    }

    /** Proxy mode: route exactly these ranges through the VPN. */
    fun addSiteRoutes(builder: VpnService.Builder, ranges: List<Range>) {
        for (r in ranges) {
            for ((ip, prefix) in rangeToCidrs(r)) {
                builder.addRoute(intToInetAddress(ip), prefix)
            }
        }
    }

    /** Bypass mode: route everything except these ranges. */
    fun addComplementRoutes(
        builder: VpnService.Builder,
        ranges: List<Range>,
        log: (String) -> Unit
    ) {
        val blocks = rangeToCidrs(Range(0L, 0xFFFFFFFFL), ranges)
        if (blocks.size > 200) {
            log("Site list produces ${blocks.size} route blocks — too fragmented, keeping it anyway")
        }
        for ((ip, prefix) in blocks) {
            builder.addRoute(intToInetAddress(ip), prefix)
        }
    }

    // ── range math ───────────────────────────────────────────────────────────

    private fun mergeRanges(input: List<Range>): List<Range> {
        if (input.isEmpty()) return emptyList()
        val sorted = input.sortedBy { it.start }
        val merged = mutableListOf<Range>()
        var cur = sorted[0]
        for (r in sorted.drop(1)) {
            if (r.start <= cur.end + 1) {
                if (r.end > cur.end) cur = Range(cur.start, r.end)
            } else {
                merged.add(cur)
                cur = r
            }
        }
        merged.add(cur)
        return merged
    }

    /**
     * Split [range] into minimal CIDR blocks, avoiding [excluded] ranges
     * (empty list = plain CIDR split of the range).
     */
    private fun rangeToCidrs(range: Range, excluded: List<Range> = emptyList()): List<Pair<Long, Int>> {
        val out = mutableListOf<Pair<Long, Int>>()
        for (r in complement(range, excluded)) {
            var s = r.start
            val e = r.end
            while (s <= e) {
                // Largest power-of-two block that is aligned at s and fits in [s, e]
                var hostBits = 0
                while (hostBits < 32) {
                    val step = 1L shl (hostBits + 1)
                    if (s % step != 0L) break
                    if (s + step - 1 > e) break
                    hostBits++
                }
                out.add(s to (32 - hostBits))
                s += 1L shl hostBits
            }
        }
        return out
    }

    /** Subtract [excluded] ranges from [range]. */
    private fun complement(range: Range, excluded: List<Range>): List<Range> {
        if (excluded.isEmpty()) return listOf(range)
        val out = mutableListOf<Range>()
        var cur = range.start
        for (r in mergeRanges(excluded.filter { it.end >= range.start && it.start <= range.end })) {
            if (r.start > cur) out.add(Range(cur, minOf(r.start - 1, range.end)))
            cur = maxOf(cur, r.end + 1)
            if (cur > range.end) break
        }
        if (cur <= range.end) out.add(Range(cur, range.end))
        return out.filter { it.start <= it.end }
    }

    // ── IPv4 helpers ─────────────────────────────────────────────────────────

    private fun isIpv4(s: String): Boolean {
        val parts = s.split(".")
        if (parts.size != 4) return false
        return parts.all {
            val v = it.toIntOrNull() ?: return@all false
            it.isNotEmpty() && v in 0..255
        }
    }

    private fun ipToInt(s: String): Long {
        if (!isIpv4(s)) throw IllegalArgumentException("not an IPv4 literal: $s")
        return bytesToInt(InetAddress.getByName(s).address)
    }

    private fun bytesToInt(b: ByteArray): Long =
        ((b[0].toLong() and 0xFF) shl 24) or
            ((b[1].toLong() and 0xFF) shl 16) or
            ((b[2].toLong() and 0xFF) shl 8) or
            (b[3].toLong() and 0xFF)

    private fun intToInetAddress(v: Long): InetAddress =
        InetAddress.getByAddress(
            byteArrayOf(
                (v shr 24).toByte(),
                (v shr 16).toByte(),
                (v shr 8).toByte(),
                v.toByte()
            )
        )

    private fun maskOf(prefix: Int): Long =
        if (prefix == 0) 0L else (-1L shl (32 - prefix)) and 0xFFFFFFFFL

    private fun maskInv(prefix: Int): Long = maskOf(prefix).inv() and 0xFFFFFFFFL
}
