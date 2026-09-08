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
 */
object SplitTunnel {

    /** Inclusive IPv4 range, uint32 stored in a Long. */
    data class Range(val start: Long, val end: Long)

    /** Load site entries (domains / IPs / CIDRs) from prefs, one per line. */
    fun siteEntries(prefs: SharedPreferences): List<String> {
        return (prefs.getString("bypass_sites", "") ?: "")
            .split("\n", ",", ";")
            .map { it.trim() }
            .filter { it.isNotEmpty() }
    }

    /** Resolve entries to IPv4 ranges. Domains are resolved via DNS. */
    fun resolveRanges(entries: List<String>, log: (String) -> Unit): List<Range> {
        val ranges = mutableListOf<Range>()
        for (entry in entries) {
            try {
                when {
                    entry.contains('/') -> {
                        val parts = entry.split("/")
                        val prefix = parts[1].toIntOrNull()
                            ?: throw IllegalArgumentException("bad prefix")
                        if (prefix < 0 || prefix > 32) throw IllegalArgumentException("prefix out of range")
                        val base = ipToInt(parts[0]) and maskOf(prefix)
                        ranges.add(Range(base, base + maskInv(prefix)))
                    }
                    isIpv4(entry) -> {
                        val ip = ipToInt(entry)
                        ranges.add(Range(ip, ip))
                    }
                    else -> {
                        // Domain: resolve to IPv4 addresses at VPN start
                        // (same approach as the desktop client)
                        var added = 0
                        for (addr in InetAddress.getAllByName(entry)) {
                            val bytes = addr.address
                            if (bytes.size == 4) {
                                val ip = bytesToInt(bytes)
                                ranges.add(Range(ip, ip))
                                added++
                            }
                        }
                        log("Site '$entry' resolved to $added IPv4 addresses")
                    }
                }
            } catch (e: Exception) {
                log("Site rule '$entry' skipped: ${e.message}")
            }
        }
        return mergeRanges(ranges)
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
