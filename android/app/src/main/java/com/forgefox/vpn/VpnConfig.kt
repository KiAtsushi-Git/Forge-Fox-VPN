package com.forgefox.vpn

import android.content.Context
import org.json.JSONArray
import org.json.JSONObject

/**
 * Builds the SSH_CONFIG_JSON passed to the native core.
 * Shared between HomeFragment (manual connect) and ForgeFoxVpnService
 * (silent restart on settings change), so a restart always uses the
 * same config as a fresh connect.
 */
object VpnConfig {

    /** Returns null when no valid ssh:// link is selected. */
    fun build(context: Context): JSONObject? {
        val prefs = context.getSharedPreferences("ForgeFoxSettings", Context.MODE_PRIVATE)
        val link = prefs.getString("selected_vless", "") ?: ""
        if (link.isEmpty() || !link.startsWith("ssh://")) return null

        val settings = JSONObject().apply {
            val savedMode = prefs.getString("proxy_mode", "tunnel") ?: "tunnel"
            put("mode", savedMode)
            put("proxy_port", prefs.getInt("proxy_port", 10808))
            put("proxy_user", prefs.getString("proxy_user", "Fox"))
            put("proxy_pass", prefs.getString("proxy_pass", "Forge"))
            put("adblock_enabled", prefs.getBoolean("adblock", false))
            val defaultAdblock = "https://raw.githubusercontent.com/lyc8503/sing-box-rules/rule-set/geosite-category-ads-all.srs"
            put("adblock_url", prefs.getString("adblock_url", defaultAdblock))
            put("split_enabled", prefs.getBoolean("split", false))
            put("split_mode", prefs.getInt("split_mode", 0))
            val apps = prefs.getString("bypass_apps", "")?.split(",")?.map { it.trim() }?.filter { it.isNotEmpty() } ?: emptyList()
            val appsArr = JSONArray()
            apps.forEach { appsArr.put(it) }
            put("bypass_apps", appsArr)
            // Wildcard zones (*.ru) → DNS interception in the native core.
            // Only meaningful in Proxy mode with split enabled.
            val splitEnabled = prefs.getBoolean("split", false)
            val splitMode = prefs.getInt("split_mode", 0)
            if (splitEnabled && splitMode == 1) {
                val zonesArr = JSONArray()
                SplitTunnel.zoneEntries(prefs).forEach { zonesArr.put(it) }
                put("dns_zones", zonesArr)
                put("dns_server_ip", "198.18.0.2")
                // Real resolver the native core forwards intercepted queries to
                // (through a protected socket, outside the VPN).
                put("dns_upstream", "8.8.8.8:53")
            }
            // Union mode: Proxy split with BOTH apps and sites/zones selected.
            // VpnService.Builder cannot express that union (an app filter
            // applies to all routes), so the TUN takes everything and the
            // native core routes each flow by destination + owner. The
            // service re-confirms this flag when it opens the TUN.
            val hasSiteEntries = SplitTunnel.siteEntries(prefs).isNotEmpty()
            put("union_mode", splitEnabled && splitMode == 1 && apps.isNotEmpty() && hasSiteEntries)
        }

        // Site rules (domains / IPs / CIDRs) — enforced as routes on the
        // VpnService.Builder; also passed to the core for logging/backup.
        val sitesArr = JSONArray()
        SplitTunnel.siteEntries(prefs).forEach { sitesArr.put(it) }
        settings.put("bypass_domains", sitesArr)

        // Parse the ssh:// link (same formats as the desktop client)
        return try {
            val withoutScheme = link.removePrefix("ssh://").substringBefore("#")
            var user = "root"
            var pass = ""
            var host = ""
            var port = 22

            val weirdRegex = Regex("^([^:@]+)@([^:@]+):([^:@]+)@([0-9]+)$")
            val weirdMatch = weirdRegex.find(withoutScheme)
            if (weirdMatch != null) {
                user = weirdMatch.groupValues[1]
                pass = weirdMatch.groupValues[2]
                host = weirdMatch.groupValues[3]
                port = weirdMatch.groupValues[4].toIntOrNull() ?: 22
            } else {
                val lastAt = withoutScheme.lastIndexOf('@')
                if (lastAt != -1) {
                    val authPart = withoutScheme.substring(0, lastAt)
                    val hostPortPart = withoutScheme.substring(lastAt + 1)
                    if (authPart.contains(':')) {
                        user = authPart.substringBefore(':')
                        pass = authPart.substringAfter(':')
                    } else if (authPart.contains('@')) {
                        user = authPart.substringBefore('@')
                        pass = authPart.substringAfter('@')
                    } else {
                        user = authPart
                    }
                    host = hostPortPart.substringBefore(':')
                    port = hostPortPart.substringAfter(':', "22").toIntOrNull() ?: 22
                } else {
                    host = withoutScheme.substringBefore(':')
                    port = withoutScheme.substringAfter(':', "22").toIntOrNull() ?: 22
                }
            }
            settings.put("link", link)
            settings.put("host", host)
            settings.put("port", port)
            settings.put("user", user)
            settings.put("pass", pass)
            settings
        } catch (e: Exception) {
            null
        }
    }
}
