$ErrorActionPreference = "Stop"

$appDir = "app/src/main/java/com/forgefox/vpn"

# MainActivity.kt
Set-Content -Path "$appDir/MainActivity.kt" -Value @"
package com.forgefox.vpn

import android.os.Bundle
import androidx.appcompat.app.AppCompatActivity
import androidx.fragment.app.Fragment
import com.google.android.material.bottomnavigation.BottomNavigationView

class MainActivity : AppCompatActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(R.layout.activity_main)

        val bottomNav = findViewById<BottomNavigationView>(R.id.bottom_nav)
        
        loadFragment(HomeFragment())

        bottomNav.setOnItemSelectedListener { item ->
            when (item.itemId) {
                R.id.nav_home -> loadFragment(HomeFragment())
                R.id.nav_servers -> loadFragment(ServersFragment())
                R.id.nav_settings -> loadFragment(SettingsFragment())
            }
            true
        }
    }

    private fun loadFragment(fragment: Fragment) {
        supportFragmentManager.beginTransaction()
            .replace(R.id.nav_host_fragment, fragment)
            .commit()
    }
}
"@

# HomeFragment.kt
Set-Content -Path "$appDir/HomeFragment.kt" -Value @"
package com.forgefox.vpn

import android.content.Intent
import android.net.VpnService
import android.os.Bundle
import android.view.LayoutInflater
import android.view.View
import android.view.ViewGroup
import android.widget.Button
import android.widget.TextView
import androidx.fragment.app.Fragment
import org.json.JSONObject

class HomeFragment : Fragment() {
    private lateinit var btnConnectWheel: Button
    private lateinit var lblStatus: TextView
    private var isConnected = false

    override fun onCreateView(
        inflater: LayoutInflater, container: ViewGroup?,
        savedInstanceState: Bundle?
    ): View? {
        val view = inflater.inflate(R.layout.fragment_home, container, false)
        btnConnectWheel = view.findViewById(R.id.btnConnectWheel)
        lblStatus = view.findViewById(R.id.lblStatus)

        btnConnectWheel.setOnClickListener {
            if (!isConnected) {
                val intent = VpnService.prepare(requireContext())
                if (intent != null) {
                    startActivityForResult(intent, 0)
                } else {
                    startVpn()
                }
            } else {
                stopVpn()
            }
        }
        return view
    }

    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        super.onActivityResult(requestCode, resultCode, data)
        if (requestCode == 0 && resultCode == android.app.Activity.RESULT_OK) {
            startVpn()
        }
    }

    private fun startVpn() {
        val prefs = requireContext().getSharedPreferences("ForgeFoxSettings", 0)
        
        val settings = JSONObject()
        settings.put("mode", "tunnel")
        settings.put("adblock_enabled", prefs.getBoolean("adblock", false))
        settings.put("split_enabled", prefs.getBoolean("split", false))
        
        val bypassDomains = prefs.getString("domains", "")?.split(",")?.map { it.trim() }?.filter { it.isNotEmpty() } ?: emptyList<String>()
        val domainsArray = org.json.JSONArray()
        bypassDomains.forEach { domainsArray.put(it) }
        settings.put("bypass_domains", domainsArray)

        // Mock VLESS (In reality, load from selected server in ServersFragment)
        val vlessLink = "vless://test@127.0.0.1:443?type=tcp"
        val vlessObj = JSONObject(Core.parseVless(vlessLink))
        settings.put("vless", vlessObj)

        if (prefs.getBoolean("doublehop", false)) {
            val entryNode = prefs.getString("entry_node", "") ?: ""
            if (entryNode.isNotEmpty()) {
                val entryObj = JSONObject(Core.parseVless(entryNode))
                settings.put("detour_vless", entryObj)
            }
        }

        val configStr = Core.buildConfig(settings.toString())
        println("Generated config: `$configStr")

        isConnected = true
        lblStatus.text = "ЗАЩИЩЕНО"
        lblStatus.setTextColor(android.graphics.Color.parseColor("#10B981"))
        btnConnectWheel.setBackgroundResource(R.drawable.circle_bg)
    }

    private fun stopVpn() {
        isConnected = false
        lblStatus.text = "ОТКЛЮЧЕНО"
        lblStatus.setTextColor(android.graphics.Color.parseColor("#A1A1AA"))
        btnConnectWheel.setBackgroundResource(R.drawable.power_wheel_off)
    }
}
"@

# SettingsFragment.kt
Set-Content -Path "$appDir/SettingsFragment.kt" -Value @"
package com.forgefox.vpn

import android.os.Bundle
import android.view.LayoutInflater
import android.view.View
import android.view.ViewGroup
import android.widget.Button
import android.widget.EditText
import android.widget.Switch
import android.widget.Toast
import androidx.fragment.app.Fragment

class SettingsFragment : Fragment() {
    override fun onCreateView(
        inflater: LayoutInflater, container: ViewGroup?,
        savedInstanceState: Bundle?
    ): View? {
        val view = inflater.inflate(R.layout.fragment_settings, container, false)
        val prefs = requireContext().getSharedPreferences("ForgeFoxSettings", 0)

        val switchAdblock = view.findViewById<Switch>(R.id.switchAdblock)
        val switchSplit = view.findViewById<Switch>(R.id.switchSplit)
        val editDomains = view.findViewById<EditText>(R.id.editDomains)
        val switchDoubleHop = view.findViewById<Switch>(R.id.switchDoubleHop)
        val editEntryNode = view.findViewById<EditText>(R.id.editEntryNode)
        val btnSave = view.findViewById<Button>(R.id.btnSave)

        switchAdblock.isChecked = prefs.getBoolean("adblock", false)
        switchSplit.isChecked = prefs.getBoolean("split", false)
        editDomains.setText(prefs.getString("domains", ""))
        switchDoubleHop.isChecked = prefs.getBoolean("doublehop", false)
        editEntryNode.setText(prefs.getString("entry_node", ""))

        btnSave.setOnClickListener {
            prefs.edit().apply {
                putBoolean("adblock", switchAdblock.isChecked)
                putBoolean("split", switchSplit.isChecked)
                putString("domains", editDomains.text.toString())
                putBoolean("doublehop", switchDoubleHop.isChecked)
                putString("entry_node", editEntryNode.text.toString())
                apply()
            }
            Toast.makeText(context, "Settings saved", Toast.LENGTH_SHORT).show()
        }

        return view
    }
}
"@

# ServersFragment.kt
Set-Content -Path "$appDir/ServersFragment.kt" -Value @"
package com.forgefox.vpn

import android.os.Bundle
import android.view.LayoutInflater
import android.view.View
import android.view.ViewGroup
import androidx.fragment.app.Fragment

class ServersFragment : Fragment() {
    override fun onCreateView(
        inflater: LayoutInflater, container: ViewGroup?,
        savedInstanceState: Bundle?
    ): View? {
        // Will implement RecyclerView logic in future
        return inflater.inflate(R.layout.fragment_servers, container, false)
    }
}
"@

Write-Host "Kotlin Scaffold completed."
