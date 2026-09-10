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

    /**
     * Hands a freshly established TUN fd to the running bridge without
     * tearing down the SSH session (used when split-tunnel routes change
     * because a DNS answer revealed new addresses). Blocks until the bridge
     * stops using the previous fd; returns false if no bridge is running.
     */
    @JvmStatic
    external fun swapTunFd(fd: Int): Boolean

    /**
     * Called from the Rust bridge thread when a DNS answer matching a
     * domain / domain_zone rule revealed addresses the route table doesn't
     * know yet. Payload: {"ips": ["1.2.3.4", ...], "query": "host"}.
     */
    @JvmStatic
    fun onDnsLearned(json: String) {
        ForgeFoxVpnService.instance?.onDnsLearned(json)
    }

    @JvmStatic
    fun protectFd(fd: Int): Boolean {
        return try {
            ForgeFoxVpnService.instance?.protect(fd) ?: false
        } catch (e: Exception) {
            false
        }
    }
}
