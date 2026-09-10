package com.forgefox.vpn

object Core {
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
}
