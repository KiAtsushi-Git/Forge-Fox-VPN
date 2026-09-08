package com.forgefox.vpn

import android.content.Intent
import android.net.VpnService
import android.os.ParcelFileDescriptor

class VpnServiceHandler : VpnService() {
    private var vpnInterface: ParcelFileDescriptor? = null

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        setupVpn()
        return START_STICKY
    }

    private fun setupVpn() {
        val builder = Builder()
        builder.addAddress("172.19.100.1", 30)
        builder.addRoute("0.0.0.0", 0)
        builder.setSession("ForgeFoxVPN")
        vpnInterface = builder.establish()
        
        // Pass vpnInterface fd to sing-box (libbox) here
    }

    override fun onDestroy() {
        super.onDestroy()
        vpnInterface?.close()
        vpnInterface = null
    }
}
