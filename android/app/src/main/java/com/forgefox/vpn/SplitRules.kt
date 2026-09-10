package com.forgefox.vpn

import android.content.Context
import android.content.SharedPreferences
import org.json.JSONArray
import org.json.JSONObject
import java.util.UUID

/**
 * Typed split-tunnel rules — same model as the desktop client
 * (windows/src-tauri/src/config/models.rs):
 *
 *  - ip          — single IPv4 address
 *  - cidr        — IPv4 subnet
 *  - domain      — exact hostname (gemini.google.com)
 *  - domain_zone — apex plus every subdomain (discord.com, *.discord.com)
 *  - app         — Android package name (the desktop analogue is an .exe path)
 *
 * Stored as JSON in SharedPreferences so rules keep their type, enabled flag
 * and order. Import/export uses the desktop plain-text format
 * (`type:value`, one per line, `#` comments) so lists can be pasted
 * between the PC and the phone by hand.
 */
object SplitRules {

    const val PREFS = "ForgeFoxSettings"
    const val KEY_RULES = "split_rules_json"

    enum class Type(val wire: String, val label: String, val hint: String) {
        IP("ip", "IP-адрес", "192.168.1.10"),
        CIDR("cidr", "Подсеть", "10.0.0.0/8"),
        DOMAIN("domain", "Домен (точно)", "gemini.google.com"),
        DOMAIN_ZONE("domain_zone", "Домен (+ поддомены)", "discord.com"),
        APP("app", "Приложение", "com.example.app");

        companion object {
            /** Wire names, plus the desktop's friendly aliases. */
            fun fromWire(s: String): Type? = when (s.trim().lowercase()) {
                "ip" -> IP
                "cidr", "subnet", "net" -> CIDR
                "domain", "site" -> DOMAIN
                "domain_zone", "domainzone", "zone" -> DOMAIN_ZONE
                "app", "exe", "process", "package" -> APP
                else -> null
            }
        }
    }

    data class Rule(
        val id: String = UUID.randomUUID().toString(),
        val type: Type,
        val value: String,
        val enabled: Boolean = true
    ) {
        /** Normalized so matching/parsing is predictable (mirrors the desktop). */
        val normalized: String
            get() {
                val v = value.trim()
                return when (type) {
                    Type.DOMAIN, Type.DOMAIN_ZONE ->
                        v.removePrefix("*.").trim('.').lowercase()
                    Type.APP -> v.trim().lowercase()
                    else -> v
                }
            }

        /** Reject values that cannot work, so bad rules fail in the UI. */
        fun validate(): String? {
            val v = normalized
            if (v.isEmpty()) return "Значение правила пусто"
            return when (type) {
                Type.IP -> if (isIpv4(v)) null else "«$v» — не похоже на IPv4-адрес"
                Type.CIDR -> {
                    val parts = v.split('/')
                    if (parts.size != 2) "«$v» — нужен формат 10.0.0.0/8"
                    else if (!isIpv4(parts[0])) "«${parts[0]}» — не похоже на IPv4-адрес"
                    else {
                        val p = parts[1].toIntOrNull()
                        if (p == null || p < 0 || p > 32) "«${parts[1]}» — некорректная длина префикса"
                        else null
                    }
                }
                Type.DOMAIN -> {
                    if (!v.contains('.') || v.startsWith('.')) "«$v» — не похоже на домен"
                    else if (v.any { it.isWhitespace() || it == '/' || it == ':' }) "«$v» содержит недопустимые символы"
                    else null
                }
                Type.DOMAIN_ZONE -> {
                    if (v.isEmpty()) "Значение правила пусто"
                    else if (v.any { it.isWhitespace() || it == '/' || it == ':' }) "«$v» содержит недопустимые символы"
                    else null
                }
                Type.APP -> if (v.matches(Regex("[A-Za-z0-9_]+(\\.[A-Za-z0-9_]+)+"))) null
                    else "«$v» — не похоже на имя пакета (com.example.app)"
            }
        }
    }

    // ── persistence ───────────────────────────────────────────────────────────

    fun load(prefs: SharedPreferences): MutableList<Rule> {
        migrateLegacy(prefs)
        val raw = prefs.getString(KEY_RULES, null) ?: return mutableListOf()
        return try {
            val arr = JSONArray(raw)
            val out = mutableListOf<Rule>()
            for (i in 0 until arr.length()) {
                val o = arr.getJSONObject(i)
                val t = Type.fromWire(o.optString("type")) ?: continue
                val value = o.optString("value") ?: continue
                if (value.isBlank()) continue
                out.add(Rule(o.optString("id", UUID.randomUUID().toString()), t, value, o.optBoolean("enabled", true)))
            }
            out
        } catch (e: Exception) {
            mutableListOf()
        }
    }

    fun save(prefs: SharedPreferences, rules: List<Rule>) {
        val arr = JSONArray()
        for (r in rules) {
            arr.put(JSONObject().apply {
                put("id", r.id)
                put("type", r.type.wire)
                put("value", r.value)
                put("enabled", r.enabled)
            })
        }
        prefs.edit().putString(KEY_RULES, arr.toString()).apply()
    }

    /**
     * One-shot upgrade from the old format (free-form `bypass_sites` lines +
     * comma-separated `bypass_apps` packages) into typed rules. Heuristics,
     * same intent as the desktop import:
     *   - contains '/' → cidr; looks like IPv4 → ip
     *   - contains exactly one dot → domain (exact), two or more dots and a
     *     known multi-label shape → domain_zone is friendlier for CDNs, but
     *     we keep "domain" for one-label-below-apex entries and zone for
     *     apex-only entries (e.g. discord.com) so subdomains match.
     */
    private fun migrateLegacy(prefs: SharedPreferences) {
        if (prefs.contains(KEY_RULES)) return

        val rules = mutableListOf<Rule>()
        val sites = (prefs.getString("bypass_sites", "") ?: "")
            .split("\n", ",", ";")
            .map { it.trim() }
            .filter { it.isNotEmpty() }
        for (s in sites) {
            val type = when {
                s.contains('/') -> Type.CIDR
                isIpv4(s) -> Type.IP
                else -> {
                    val labels = s.trim('.').split('.').filter { it.isNotEmpty() }.size
                    // Two labels (discord.com) behave best as a zone: subdomains
                    // of CDNs would slip past an exact match. Deeper names
                    // (gemini.google.com) stay exact.
                    if (labels <= 2) Type.DOMAIN_ZONE else Type.DOMAIN
                }
            }
            rules.add(Rule(type = type, value = s))
        }

        val apps = (prefs.getString("bypass_apps", "") ?: "")
            .split(",")
            .map { it.trim() }
            .filter { it.isNotEmpty() }
        for (a in apps) rules.add(Rule(type = Type.APP, value = a))

        if (rules.isNotEmpty()) {
            save(prefs, rules)
            // Rules now own this state; keep the legacy keys only as a backup.
        }
    }

    // ── export / import (desktop format) ─────────────────────────────────────

    /** Plain-text export: one `type:value` per line, `#` comments allowed. */
    fun export(rules: List<Rule>): String {
        val sb = StringBuilder("# ForgeFoxVPN split-tunnel rules\n# формат: тип:значение\n")
        for (r in rules) {
            if (!r.enabled) sb.append("# (выключено) ")
            sb.append("${r.type.wire}:${r.value}\n")
        }
        return sb.toString()
    }

    /**
     * Parse a pasted list (the PC export). App rules whose value is a Windows
     * path are skipped — an .exe path can never match an Android package.
     * Returns the parsed rules plus per-line errors.
     */
    fun parseImport(text: String): Pair<List<Rule>, List<String>> {
        val rules = mutableListOf<Rule>()
        val errors = mutableListOf<String>()
        for ((idx, raw) in text.lines().withIndex()) {
            val line = raw.trim()
            if (line.isEmpty() || line.startsWith("#")) continue
            // Desktop export writes disabled rules as "# (выключено) type:value"
            // — already skipped by the comment check above. Detect that and
            // import it back as disabled for round-tripping.
            val disabled = line.startsWith("(выключено) ")
            val body = if (disabled) line.removePrefix("(выключено) ") else line

            val colon = body.indexOf(':')
            if (colon <= 0) {
                errors.add("Строка ${idx + 1}: не разобрать «$line»")
                continue
            }
            val type = Type.fromWire(body.substring(0, colon))
            if (type == null) {
                errors.add("Строка ${idx + 1}: неизвестный тип «${body.substring(0, colon)}»")
                continue
            }
            val value = body.substring(colon + 1).trim()
            if (value.isEmpty()) {
                errors.add("Строка ${idx + 1}: пустое значение")
                continue
            }
            if (type == Type.APP && (value.contains('\\') || value.contains('/'))) {
                // A Windows path from the PC export — meaningless on Android.
                continue
            }
            val rule = Rule(type = type, value = value, enabled = !disabled)
            rule.validate()?.let { errors.add("Строка ${idx + 1}: $it") }
            rules.add(rule)
        }
        return rules to errors
    }

    /**
     * Import parsed rules into storage. Deduplicated against existing rules.
     * Returns the number of rules actually added.
     */
    fun import(prefs: SharedPreferences, rules: List<Rule>, replace: Boolean): Int {
        val current = if (replace) mutableListOf() else load(prefs)
        var added = 0
        for (r in rules) {
            val dup = current.any { it.type == r.type && it.normalized == r.normalized }
            if (!dup) {
                current.add(r)
                added++
            }
        }
        save(prefs, current)
        return added
    }

    // ── accessors for the VPN service ────────────────────────────────────────

    fun enabledRules(ctx: Context): List<Rule> =
        load(ctx.getSharedPreferences(PREFS, Context.MODE_PRIVATE)).filter { it.enabled }

    fun appPackages(rules: List<Rule>): List<String> =
        rules.filter { it.type == Type.APP }.map { it.normalized }

    fun domains(rules: List<Rule>): List<String> =
        rules.filter { it.type == Type.DOMAIN }.map { it.normalized }

    fun zones(rules: List<Rule>): List<String> =
        rules.filter { it.type == Type.DOMAIN_ZONE }.map { it.normalized }

    // ── helpers ───────────────────────────────────────────────────────────────

    fun isIpv4(s: String): Boolean {
        val parts = s.split(".")
        if (parts.size != 4) return false
        return parts.all {
            val v = it.toIntOrNull() ?: return@all false
            it.isNotEmpty() && v in 0..255
        }
    }
}
