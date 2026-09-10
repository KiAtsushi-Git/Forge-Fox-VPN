package com.forgefox.vpn

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Intent
import android.net.VpnService
import android.os.Build
import android.os.ParcelFileDescriptor
import android.util.Log

class ForgeFoxVpnService : VpnService() {

    private var tunFd: Int = -1
    private var trafficTimer: java.util.Timer? = null
    private var txBytes: Long = 0
    private var rxBytes: Long = 0

    companion object {
        private const val TUN_MTU = 1400
        @Volatile var isRunning = false
        var instance: ForgeFoxVpnService? = null
        var connectionState = ""
        // Incremented on every start; lets an old connect-loop exit when the
        // VPN is silently restarted with new settings instead of reconnecting.
        @Volatile var generation = 0
        val logs = java.util.concurrent.CopyOnWriteArrayList<String>()
        fun addLog(msg: String) {
            logs.add(0, msg)
            if (logs.size > 200) logs.removeAt(logs.size - 1)
        }
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        when (intent?.action) {
            "STOP_VPN" -> {
                addLog("Received STOP_VPN")
                performStop()
                return START_NOT_STICKY
            }
            "START_VPN_SILENT" -> {
                // Settings changed while running: restart with the saved config
                if (!isRunning) return START_NOT_STICKY
                addLog("Settings changed — restarting VPN...")
                performStop()
                android.os.Handler(android.os.Looper.getMainLooper()).postDelayed({
                    try {
                        val cfg = VpnConfig.build(this)
                        if (cfg != null) {
                            startService(Intent(this, ForgeFoxVpnService::class.java).apply {
                                action = VpnService.SERVICE_INTERFACE
                                putExtra("SSH_LINK", cfg.optString("link"))
                                putExtra("SSH_CONFIG_JSON", cfg.toString())
                            })
                        } else {
                            addLog("Restart skipped: no SSH server selected")
                        }
                    } catch (e: Exception) {
                        addLog("Restart failed: ${e.message}")
                    }
                }, 800)
                return START_NOT_STICKY
            }
        }

        val sshLink = intent?.getStringExtra("SSH_LINK")
        val sshConfigJson = intent?.getStringExtra("SSH_CONFIG_JSON") ?: ""
        
        if (sshLink == null) return START_NOT_STICKY
        
        instance = this

        try {
            createNotificationChannel()
            val notification = buildNotification("Подключение...", true)
            if (Build.VERSION.SDK_INT >= 34) {
                startForeground(1, notification, android.content.pm.ServiceInfo.FOREGROUND_SERVICE_TYPE_SPECIAL_USE)
            } else {
                startForeground(1, notification)
            }
        } catch (e: Exception) {
            addLog("startForeground failed: ${e.message}, continuing without it")
        }

        Thread {
            try {
                addLog("Starting VPN service...")
                addLog("Starting SSH VPN for $sshLink...")
                
                val randomX = (10..250).random()
                val randomY = (10..250).random()
                val clientIp = "10.$randomX.$randomY.2"
                val serverIp = "10.$randomX.$randomY.1"
                
                val fd = openTunCustom(clientIp)
                if (fd == -1) {
                    addLog("Failed to open TUN for SSH VPN")
                    throw Exception("Failed to open TUN for SSH")
                }
                addLog("TUN opened successfully (fd=$fd, client=$clientIp, server=$serverIp)")
                
                isRunning = true
                val myGen = ++generation
                updateNotification("Подключено ✓", true)
                startTrafficTimer()

                val json = org.json.JSONObject(sshConfigJson)
                json.put("server_tun_ip", serverIp)
                // The native core passes this to the server-side bridge
                // (forgefox-bridge / python fallback) so both TUN ends agree.
                json.put("mtu", TUN_MTU)
                val finalSettingsJson = json.toString()

                while (isRunning && generation == myGen) {
                    addLog("Starting SSH VPN core (blocking)...")
                    Core.startSshVpn(fd, finalSettingsJson)

                    if (!isRunning || generation != myGen) {
                        addLog("SSH VPN core finished and VPN is manually stopped.")
                        break
                    } else {
                        addLog("VPN connection dropped! Reconnecting in 3 seconds...")
                        updateNotification("Переподключение...", true)
                        Thread.sleep(3000)
                        if (isRunning && generation == myGen) {
                            updateNotification("Подключено ✓", true)
                        }
                    }
                }
                
                isRunning = false
                updateNotification("Отключено", false)

            } catch (e: Exception) {
                addLog("ERROR: ${Log.getStackTraceString(e)}")
                isRunning = false
                try { stopForeground(true) } catch (_: Exception) {}
                stopSelf()
            }
        }.start()

        return START_STICKY
    }

    private fun createNotificationChannel() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            val channel = NotificationChannel(
                "forgefox_vpn", "ForgeFox VPN", NotificationManager.IMPORTANCE_LOW
            )
            channel.description = "VPN статус"
            channel.setShowBadge(false)
            getSystemService(NotificationManager::class.java).createNotificationChannel(channel)
        }
    }

    private fun buildNotification(text: String, isConnected: Boolean): Notification {
        val builder = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            Notification.Builder(this, "forgefox_vpn")
        } else {
            @Suppress("DEPRECATION")
            Notification.Builder(this)
        }

        builder.setContentTitle("ForgeFox VPN")
            .setContentText(text)
            .setSmallIcon(android.R.drawable.ic_lock_lock)
            .setOngoing(isConnected)

        if (isConnected) {
            val stopIntent = Intent(this, ForgeFoxVpnService::class.java).apply {
                action = "STOP_VPN"
            }
            val stopPending = PendingIntent.getService(
                this, 0, stopIntent,
                PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT
            )
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.M) {
                builder.addAction(
                    Notification.Action.Builder(
                        null, "Отключить", stopPending
                    ).build()
                )
            }
        }

        return builder.build()
    }

    private fun updateNotification(text: String, isConnected: Boolean) {
        connectionState = text
        try {
            val notification = buildNotification(text, isConnected)
            val nm = getSystemService(NotificationManager::class.java)
            nm.notify(1, notification)
        } catch (e: Exception) {
            addLog("updateNotification error: ${e.message}")
        }
    }

    private fun startTrafficTimer() {
        trafficTimer?.cancel()
        txBytes = android.net.TrafficStats.getTotalTxBytes()
        rxBytes = android.net.TrafficStats.getTotalRxBytes()

        trafficTimer = java.util.Timer().also {
            it.scheduleAtFixedRate(object : java.util.TimerTask() {
                override fun run() {
                    if (!isRunning) {
                        cancel()
                        return
                    }
                    val newTx = android.net.TrafficStats.getTotalTxBytes()
                    val newRx = android.net.TrafficStats.getTotalRxBytes()
                    val txDiff = newTx - txBytes
                    val rxDiff = newRx - rxBytes

                    val text = "↑ ${formatBytes(txDiff)} ↓ ${formatBytes(rxDiff)}"
                    updateNotification(text, true)
                }
            }, 2000, 2000)
        }
    }

    private fun formatBytes(bytes: Long): String {
        return when {
            bytes < 1024 -> "${bytes} B"
            bytes < 1024 * 1024 -> "${bytes / 1024} KB"
            bytes < 1024 * 1024 * 1024 -> "${"%.1f".format(bytes / (1024.0 * 1024.0))} MB"
            else -> "${"%.2f".format(bytes / (1024.0 * 1024.0 * 1024.0))} GB"
        }
    }

    fun performStop() {
        addLog("performStop()")
        isRunning = false
        trafficTimer?.cancel()
        trafficTimer = null

        Thread {
            try {
                Core.stopSshVpn()
                addLog("SSH VPN core stopped")
            } catch (e: Exception) {}

            closeTunFd()

            instance = null

            try {
                stopForeground(true)
            } catch (_: Exception) {}

            stopSelf()
            addLog("stopSelf() called")
        }.start()
    }

    private fun closeTunFd() {
        if (tunFd != -1) {
            try {
                ParcelFileDescriptor.adoptFd(tunFd).close()
                addLog("tun fd $tunFd closed")
            } catch (e: Exception) {
                addLog("closeTunFd error: ${e.message}")
            }
            tunFd = -1
        }
    }

    override fun onRevoke() {
        addLog("onRevoke()")
        performStop()
        super.onRevoke()
    }

    override fun onDestroy() {
        addLog("onDestroy()")
        isRunning = false
        trafficTimer?.cancel()
        try { Core.stopSshVpn() } catch (_: Exception) {}
        closeTunFd()
        instance = null
        super.onDestroy()
    }

    private fun openTunCustom(clientIp: String): Int {
        try {
            val builder = Builder()
            builder.addAddress(clientIp, 24)
            try { builder.addAddress("fd00:1:2:3::2", 64) } catch (e: Exception) {}
            builder.setMtu(TUN_MTU)
            builder.setSession("SSH VPN")
            
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.LOLLIPOP_MR1) {
                builder.setUnderlyingNetworks(null)
            }
            
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
                builder.setMetered(false)
            }
            
            val prefs = getSharedPreferences("ForgeFoxSettings", android.content.Context.MODE_PRIVATE)
            val splitEnabled = prefs.getBoolean("split", false)
            val splitMode = prefs.getInt("split_mode", 0) // 0: bypass, 1: proxy

            // Wildcard zones (*.ru) need DNS interception: the native core runs
            // a DNS server on this IP inside the TUN and maps matched domains
            // to fake IPs from the 198.18.0.0/15 pool (routed below).
            val zones = SplitTunnel.zoneEntries(prefs)
            val dnsIntercept = splitEnabled && splitMode == 1 && zones.isNotEmpty()
            val dnsServerIp = "198.18.0.2"

            if (dnsIntercept) {
                builder.addDnsServer(dnsServerIp)
                addLog("DNS interception ON: zones ${zones.joinToString(", ")}")
            } else {
                builder.addDnsServer("8.8.8.8")
                builder.addDnsServer("1.1.1.1")
                try { builder.addDnsServer("2001:4860:4860::8888") } catch (e: Exception) {}
            }

            if (splitEnabled) {
                // Apps
                val apps = prefs.getString("bypass_apps", "")?.split(",")?.map { it.trim() }?.filter { it.isNotEmpty() } ?: emptyList()

                // Sites (domains / IPs / CIDRs) — resolved to IPv4 ranges.
                // Wildcard zones are excluded: they are enforced by the
                // native DNS interceptor instead of static routes.
                val sites = SplitTunnel.resolveRanges(SplitTunnel.exactEntries(prefs)) { addLog(it) }

                if (splitMode == 1) {
                    // Proxy mode: Route ONLY the selected apps and/or sites
                    var addedApps = 0
                    for (pkg in apps) {
                        try {
                            builder.addAllowedApplication(pkg)
                            addedApps++
                        } catch (e: Exception) {
                            addLog("App $pkg not found")
                        }
                    }
                    // Selected apps get their full traffic through the VPN
                    if (addedApps > 0) builder.addRoute("0.0.0.0", 0)
                    // Selected sites get routed through the VPN
                    SplitTunnel.addSiteRoutes(builder, sites)
                    // Fake-IP pool for wildcard zones: every fake IP the DNS
                    // interceptor hands out lives here, so newly resolved
                    // zone domains are routed instantly without a TUN rebuild.
                    if (dnsIntercept) builder.addRoute("198.18.0.0", 15)

                    if (addedApps == 0 && sites.isEmpty() && !dnsIntercept) {
                        addLog("Proxy mode with an empty list - no traffic will be routed!")
                    } else {
                        addLog("Split tunneling (Proxy): $addedApps apps + ${sites.size} site ranges + ${zones.size} zones through VPN.")
                    }
                } else {
                    // Bypass mode: Route EVERYTHING EXCEPT selected apps and sites
                    var excludedApps = 0
                    for (pkg in apps) {
                        try {
                            builder.addDisallowedApplication(pkg)
                            excludedApps++
                        } catch (e: Exception) {
                            addLog("App $pkg not found")
                        }
                    }

                    if (sites.isEmpty()) {
                        builder.addRoute("0.0.0.0", 0)
                        addLog("Split tunneling (Bypass): excluding $excludedApps apps.")
                    } else {
                        // 0.0.0.0/0 netsplit around the excluded site ranges;
                        // uncovered destinations fall through to the physical network
                        SplitTunnel.addComplementRoutes(builder, sites) { addLog(it) }
                        addLog("Split tunneling (Bypass): excluding $excludedApps apps and ${sites.size} site ranges.")
                    }
                }
            } else {
                builder.addRoute("0.0.0.0", 0)
            }
            
            val pfd = builder.establish()
            if (pfd != null) {
                tunFd = pfd.detachFd()
                addLog("openTunCustom: established fd $tunFd")
                return tunFd
            }
        } catch (e: Exception) {
            addLog("openTunCustom error: ${e.message}")
        }
        return -1
    }
}
