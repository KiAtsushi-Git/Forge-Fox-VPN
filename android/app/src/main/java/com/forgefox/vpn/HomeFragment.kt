package com.forgefox.vpn

import android.animation.ObjectAnimator
import android.animation.ValueAnimator
import android.content.Context
import android.content.Intent
import android.net.VpnService
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.view.LayoutInflater
import android.view.View
import android.view.ViewGroup
import android.view.animation.LinearInterpolator
import android.widget.ImageView
import android.widget.LinearLayout
import android.widget.TextView
import android.widget.Toast
import androidx.appcompat.widget.SwitchCompat
import androidx.fragment.app.Fragment
import org.json.JSONArray
import org.json.JSONObject

class HomeFragment : Fragment() {

    private lateinit var btnConnectWheel: android.widget.FrameLayout
    private lateinit var lblStatus: TextView
    private lateinit var imgGlow: View
    private lateinit var btnSelectServer: LinearLayout
    private lateinit var lblSelectedServer: TextView
    private lateinit var switchExceptions: SwitchCompat

    private var isConnected = false
    private var glowAnimator: ObjectAnimator? = null
    private var pulseAnimator: ValueAnimator? = null
    private val handler = Handler(Looper.getMainLooper())
    private var statusPoller: Runnable? = null

    override fun onCreateView(
        inflater: LayoutInflater, container: ViewGroup?,
        savedInstanceState: Bundle?
    ): View? {
        val view = inflater.inflate(R.layout.fragment_home, container, false)

        btnConnectWheel = view.findViewById(R.id.btnConnectWheel)
        lblStatus       = view.findViewById(R.id.lblStatus)
        imgGlow         = view.findViewById(R.id.imgGlow)
        btnSelectServer = view.findViewById(R.id.btnSelectServer)
        lblSelectedServer = view.findViewById(R.id.lblSelectedServer)
        switchExceptions = view.findViewById(R.id.switchExceptions)

        val prefs = requireContext().getSharedPreferences("ForgeFoxSettings", Context.MODE_PRIVATE)

        switchExceptions.isChecked = prefs.getBoolean("split", false)
        switchExceptions.setOnCheckedChangeListener { _, c ->
            prefs.edit().putBoolean("split", c).apply()
            if (ForgeFoxVpnService.isRunning) {
                requireContext().startService(android.content.Intent(requireContext(), ForgeFoxVpnService::class.java).apply { action = "START_VPN_SILENT" })
            }
        }

        // Go to servers tab when tapping server selector
        btnSelectServer.setOnClickListener {
            requireActivity()
                .findViewById<com.google.android.material.bottomnavigation.BottomNavigationView>(R.id.bottom_nav)
                .selectedItemId = R.id.nav_servers
        }

        // Main power button with press animation
        btnConnectWheel.setOnClickListener { view ->
            view.animate().scaleX(0.88f).scaleY(0.88f).setDuration(100).withEndAction {
                view.animate().scaleX(1f).scaleY(1f).setDuration(150).start()

                if (!isConnected) {
                    val savedLink = prefs.getString("selected_vless", "") ?: ""
                    if (savedLink.isEmpty() || !savedLink.startsWith("ssh://")) {
                        Toast.makeText(context, "Сначала добавьте SSH сервер! →", Toast.LENGTH_SHORT).show()
                        return@withEndAction
                    }
                    val permIntent = VpnService.prepare(requireContext())
                    if (permIntent != null) {
                        @Suppress("DEPRECATION")
                        startActivityForResult(permIntent, VPN_PERMISSION_CODE)
                    } else {
                        startVpn()
                    }
                } else {
                    stopVpn()
                }
            }.start()
        }

        return view
    }

    override fun onResume() {
        super.onResume()
        syncState()
        updateSelectedServerName()
        startStatusPoller()
    }

    override fun onPause() {
        super.onPause()
        stopStatusPoller()
    }

    private fun startStatusPoller() {
        statusPoller = object : Runnable {
            override fun run() {
                val running = ForgeFoxVpnService.isRunning
                if (running != isConnected) {
                    isConnected = running
                    if (running) updateUiConnected() else updateUiDisconnected()
                }
                
                val state = ForgeFoxVpnService.connectionState
                if (state.isNotEmpty() && isConnected) {
                    lblStatus.text = state
                    if (state == "Подключено ✓") {
                        lblStatus.setTextColor(android.graphics.Color.parseColor("#FF6B00"))
                    } else {
                        lblStatus.setTextColor(android.graphics.Color.parseColor("#EAB308"))
                    }
                }
                
                handler.postDelayed(this, 500)
            }
        }
        handler.postDelayed(statusPoller!!, 500)
    }

    private fun stopStatusPoller() {
        statusPoller?.let { handler.removeCallbacks(it) }
        statusPoller = null
    }

    private fun syncState() {
        isConnected = ForgeFoxVpnService.isRunning
        if (isConnected) updateUiConnected() else updateUiDisconnected()
    }

    private fun updateSelectedServerName() {
        val prefs = requireContext().getSharedPreferences("ForgeFoxSettings", Context.MODE_PRIVATE)
        val savedLink = prefs.getString("selected_vless", "") ?: ""
        if (savedLink.isEmpty()) {
            lblSelectedServer.text = "Выбрать сервер"
            return
        }

        val serversPrefs = requireContext().getSharedPreferences("ForgeFoxServers", Context.MODE_PRIVATE)
        val data = serversPrefs.getString("servers", "[]") ?: "[]"
        var foundName = savedLink.substringAfterLast("#", "").ifEmpty { "Сервер" }
        try {
            val arr = JSONArray(data)
            outer@ for (i in 0 until arr.length()) {
                val obj = arr.getJSONObject(i)
                if (obj.optString("type") == "subscription") {
                    val nodes = obj.optJSONArray("nodes") ?: continue
                    for (j in 0 until nodes.length()) {
                        val node = nodes.getJSONObject(j)
                        if (node.optString("link") == savedLink) {
                            foundName = node.optString("name", foundName)
                            break@outer
                        }
                    }
                } else if (obj.optString("link") == savedLink) {
                    foundName = obj.optString("name", foundName)
                    break
                }
            }
        } catch (_: Exception) {}
        lblSelectedServer.text = foundName
    }

    private fun updateUiConnected() {
        lblStatus.text = "ЗАЩИЩЕНО"
        lblStatus.setTextColor(android.graphics.Color.parseColor("#FF6B00"))
        btnConnectWheel.setBackgroundResource(R.drawable.power_btn_bg)
        btnConnectWheel.isSelected = true
        val icon = btnConnectWheel.getChildAt(0) as? android.widget.ImageView
        icon?.clearColorFilter()
        
        imgGlow.alpha = 0.85f
        glowAnimator?.cancel()
        // Instead of rotating the image, let's pulse the glow ring scale
        glowAnimator = ObjectAnimator.ofPropertyValuesHolder(
            imgGlow,
            android.animation.PropertyValuesHolder.ofFloat(View.SCALE_X, 1.0f, 1.5f, 1.0f),
            android.animation.PropertyValuesHolder.ofFloat(View.SCALE_Y, 1.0f, 1.5f, 1.0f),
            android.animation.PropertyValuesHolder.ofFloat(View.ALPHA, 0f, 0.4f, 0f)
        ).apply {
            duration = 2000
            repeatCount = ValueAnimator.INFINITE
            interpolator = android.view.animation.AccelerateDecelerateInterpolator()
            start()
        }

        // Subtle pulse on button
        pulseAnimator?.cancel()
        pulseAnimator = ValueAnimator.ofFloat(1f, 1.05f, 1f).apply {
            duration = 2000
            repeatCount = ValueAnimator.INFINITE
            interpolator = android.view.animation.AccelerateDecelerateInterpolator()
            addUpdateListener {
                val v = it.animatedValue as Float
                btnConnectWheel.scaleX = v
                btnConnectWheel.scaleY = v
            }
            start()
        }
    }

    private fun updateUiDisconnected() {
        lblStatus.text = "НЕ ЗАЩИЩЕНО"
        lblStatus.setTextColor(android.graphics.Color.parseColor("#52525B"))
        btnConnectWheel.setBackgroundResource(R.drawable.power_btn_bg)
        btnConnectWheel.isSelected = false
        val icon = btnConnectWheel.getChildAt(0) as? android.widget.ImageView
        icon?.setColorFilter(android.graphics.Color.parseColor("#71717A"))

        glowAnimator?.cancel()
        glowAnimator = null
        pulseAnimator?.cancel()
        pulseAnimator = null
        btnConnectWheel.scaleX = 1f
        btnConnectWheel.scaleY = 1f

        imgGlow.animate().alpha(0f).setDuration(400).start()
    }

    @Suppress("DEPRECATION")
    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        super.onActivityResult(requestCode, resultCode, data)
        if (requestCode == VPN_PERMISSION_CODE && resultCode == android.app.Activity.RESULT_OK) {
            startVpn()
        }
    }

    override fun onViewCreated(view: View, savedInstanceState: Bundle?) {
        super.onViewCreated(view, savedInstanceState)
        
        // Request notification permission for Android 13+
        if (android.os.Build.VERSION.SDK_INT >= android.os.Build.VERSION_CODES.TIRAMISU) {
            if (requireContext().checkSelfPermission(android.Manifest.permission.POST_NOTIFICATIONS) != android.content.pm.PackageManager.PERMISSION_GRANTED) {
                requestPermissions(arrayOf(android.Manifest.permission.POST_NOTIFICATIONS), 101)
            }
        }

        if (arguments?.getBoolean("AUTO_START_VPN") == true) {
            val prefs = requireContext().getSharedPreferences("ForgeFoxSettings", Context.MODE_PRIVATE)
            val savedLink = prefs.getString("selected_vless", "") ?: ""
            if (savedLink.isNotEmpty() && savedLink.startsWith("ssh://")) {
                val permIntent = VpnService.prepare(requireContext())
                if (permIntent != null) {
                    @Suppress("DEPRECATION")
                    startActivityForResult(permIntent, VPN_PERMISSION_CODE)
                } else {
                    startVpn()
                }
            }
        }
    }

    fun startVpn() {
        val prefs = requireContext().getSharedPreferences("ForgeFoxSettings", Context.MODE_PRIVATE)
        val savedLink = prefs.getString("selected_vless", "") ?: ""

        if (savedLink.isEmpty() || !savedLink.startsWith("ssh://")) {
            Toast.makeText(context, "Сначала выберите сервер (SSH)!", Toast.LENGTH_SHORT).show()
            return
        }

        // Show connecting state immediately
        lblStatus.text = "ПОДКЛЮЧЕНИЕ..."
        lblStatus.setTextColor(android.graphics.Color.parseColor("#FF8C00"))

        val settings = VpnConfig.build(requireContext())
        if (settings == null) {
            Toast.makeText(context, "Ошибка парсинга ссылки сервера", Toast.LENGTH_SHORT).show()
            updateUiDisconnected()
            return
        }

        try {
            val intent = Intent(requireContext(), ForgeFoxVpnService::class.java).apply {
                action = VpnService.SERVICE_INTERFACE
                putExtra("SSH_LINK", savedLink)
                putExtra("SSH_CONFIG_JSON", settings.toString())
            }
            requireContext().startService(intent)
            isConnected = true
            updateUiConnected()
        } catch (e: Exception) {
            Toast.makeText(context, "Ошибка: ${e.message}", Toast.LENGTH_SHORT).show()
            updateUiDisconnected()
        }
    }

    private fun stopVpn() {
        val svc = ForgeFoxVpnService.instance
        if (svc != null) {
            svc.performStop()
        } else {
            try {
                val intent = Intent(requireContext(), ForgeFoxVpnService::class.java).apply {
                    action = "STOP_VPN"
                }
                requireContext().startService(intent)
            } catch (e: Exception) {
                e.printStackTrace()
            }
        }
        isConnected = false
        updateUiDisconnected()
    }

    companion object {
        private const val VPN_PERMISSION_CODE = 100
    }
}
