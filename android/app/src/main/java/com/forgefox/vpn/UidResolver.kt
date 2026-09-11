package com.forgefox.vpn

import android.content.Context
import android.net.ConnectivityManager
import android.os.Build
import java.io.File
import java.net.InetAddress

/**
 * Connection-owner lookup for union-mode split tunneling.
 *
 * The native core routes every TUN flow by (destination, connection owner).
 * To find the owner of a connection it calls Core.getConnectionOwner, which
 * lands here:
 *  - Android 10+: ConnectivityManager.getConnectionOwnerUid (the sanctioned
 *    replacement for /proc/net, which was hidden from apps in Q);
 *  - Android 8/9: /proc/net/{tcp,udp} parsing.
 */
object UidResolver {

    /** uid → "is a selected app" cache (uids are stable for a boot). */
    private val uidCache = java.util.concurrent.ConcurrentHashMap<Int, Boolean>()

    /** Parsed bypass_apps with a short TTL so toggle changes are picked up. */
    @Volatile
    private var appsCache: Pair<Long, Set<String>>? = null
    private const val APPS_TTL_MS = 1000L

    /** Convert an IPv4 address held in the low 32 bits of an Int to bytes. */
    private fun intToBytes(v: Int): ByteArray = byteArrayOf(
        (v ushr 24).toByte(),
        (v ushr 16).toByte(),
        (v ushr 8).toByte(),
        v.toByte()
    )

    private fun intToInetAddress(v: Int): InetAddress = InetAddress.getByAddress(intToBytes(v))

    /**
     * The uid owning (proto, src:sport → dst:dport), or -1 when unknown.
     * Addresses are IPv4 as (possibly negative) ints, network order.
     */
    fun getConnectionOwner(
        context: Context,
        proto: Int,
        srcIp: Int,
        sport: Int,
        dstIp: Int,
        dport: Int
    ): Int {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            return try {
                val cm = context.getSystemService(ConnectivityManager::class.java) ?: return -1
                val local = java.net.InetSocketAddress(intToInetAddress(srcIp), sport)
                val remote = java.net.InetSocketAddress(intToInetAddress(dstIp), dport)
                cm.getConnectionOwnerUid(proto, local, remote)
            } catch (e: Exception) {
                -1
            }
        }
        return lookupProcNet(proto, srcIp, sport, dstIp, dport)
    }

    /**
     * True when [uid] belongs to one of the apps selected for the VPN
     * (bypass_apps in Proxy mode). Our own uid is never selected: our
     * unprotected sockets must bypass to avoid routing loops.
     */
    fun isAppSelected(uid: Int, context: Context): Boolean {
        if (uid <= 0) return false
        if (uid == android.os.Process.myUid()) return false
        uidCache[uid]?.let { return it }

        val selected = selectedPackages(context)
        val result = try {
            val pkgs = context.packageManager.getPackagesForUid(uid)
            pkgs != null && pkgs.any { it in selected }
        } catch (e: Exception) {
            false
        }
        uidCache[uid] = result
        return result
    }

    private fun selectedPackages(context: Context): Set<String> {
        val now = System.currentTimeMillis()
        appsCache?.let { (ts, pkgs) ->
            if (now - ts < APPS_TTL_MS) return pkgs
        }
        val prefs = context.getSharedPreferences("ForgeFoxSettings", Context.MODE_PRIVATE)
        val pkgs = (prefs.getString("bypass_apps", "") ?: "")
            .split(",").map { it.trim() }.filter { it.isNotEmpty() }.toSet()
        appsCache = now to pkgs
        return pkgs
    }

    /**
     * /proc/net parsing for Android 8.x / 9: match the socket by its
     * local+remote address pair. The kernel prints IPv4 addresses as
     * little-endian hex ("0100A8C0" = 192.168.0.1).
     */
    private fun lookupProcNet(proto: Int, srcIp: Int, sport: Int, dstIp: Int, dport: Int): Int {
        val file = when (proto) {
            6 -> "/proc/net/tcp"
            17 -> "/proc/net/udp"
            else -> return -1
        }
        val b = intToBytes(dstIp)
        val remoteHex = "%02X%02X%02X%02X:%04X".format(b[3], b[2], b[1], b[0], dport)
        val b2 = intToBytes(srcIp)
        val localHex = "%02X%02X%02X%02X:%04X".format(b2[3], b2[2], b2[1], b2[0], sport)
        try {
            // /proc files report size 0; read line by line, capped defensively.
            File(file).bufferedReader().useLines { lines ->
                for (line in lines) {
                    val cols = line.trim().split(Regex("\\s+"))
                    if (cols.size > 7) {
                        // 0:sl 1:local 2:remote 3:st ... 7:uid
                        if (cols[1].equals(localHex, true) && cols[2].equals(remoteHex, true)) {
                            return cols[7].toIntOrNull() ?: -1
                        }
                    }
                }
            }
        } catch (e: Exception) {
            // Unreadable or hidden (Android 10+): fall back to "unknown".
        }
        return -1
    }
}
