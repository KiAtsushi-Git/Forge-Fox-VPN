package com.forgefox.vpn

import android.content.Context

object Core {
    /**
     * Application context, set by ForgeFoxVpnService before the native core
     * starts; lets the JNI callbacks below use PackageManager /
     * ConnectivityManager from any thread.
     */
    @Volatile
    var appContext: Context? = null

    init {
        System.loadLibrary("rust_core")
    }

    /**
     * Starts custom SSH L3 VPN core.
     */
    @JvmStatic
    external fun startSshVpn(fd: Int, settingsJson: String)

    /**
     * Stops custom SSH L3 VPN core.
     */
    @JvmStatic
    external fun stopSshVpn()

    @JvmStatic
    fun protectFd(fd: Int): Boolean {
        return try {
            ForgeFoxVpnService.instance?.protect(fd) ?: false
        } catch (e: Exception) {
            false
        }
    }

    /**
     * Called from the native core (union-mode split tunneling): the uid
     * owning the connection (proto, src:sport → dst:dport), or -1.
     * Addresses are IPv4 ints in network order.
     */
    @JvmStatic
    fun getConnectionOwner(proto: Int, srcIp: Int, sport: Int, dstIp: Int, dport: Int): Int {
        val ctx = appContext ?: return -1
        return UidResolver.getConnectionOwner(ctx, proto, srcIp, sport, dstIp, dport)
    }

    /**
     * Called from the native core: true when [uid] belongs to one of the
     * apps selected for the VPN (Proxy split mode).
     */
    @JvmStatic
    fun isAppSelected(uid: Int): Boolean {
        val ctx = appContext ?: return false
        return UidResolver.isAppSelected(uid, ctx)
    }
}
