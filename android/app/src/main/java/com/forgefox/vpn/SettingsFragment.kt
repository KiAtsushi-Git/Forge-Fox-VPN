package com.forgefox.vpn

import android.app.AlertDialog
import android.content.Context
import android.content.pm.ApplicationInfo
import android.content.pm.PackageManager
import android.graphics.drawable.Drawable
import android.os.Bundle
import android.view.LayoutInflater
import android.view.View
import android.view.ViewGroup
import android.widget.ImageView
import android.widget.TextView
import androidx.appcompat.widget.SwitchCompat
import androidx.fragment.app.Fragment
import androidx.recyclerview.widget.LinearLayoutManager
import androidx.recyclerview.widget.RecyclerView

class SettingsFragment : Fragment() {

    override fun onCreateView(
        inflater: LayoutInflater, container: ViewGroup?,
        savedInstanceState: Bundle?
    ): View? {
        val view = inflater.inflate(R.layout.fragment_settings, container, false)
        val prefs = requireContext().getSharedPreferences("ForgeFoxSettings", 0)

        // --- Split Tunnel Config ---
        val switchSplit = view.findViewById<SwitchCompat>(R.id.switchSplit)
        val btnSplitMode = view.findViewById<View>(R.id.btnSplitMode)
        val lblSplitMode = view.findViewById<TextView>(R.id.lblSplitMode)

        switchSplit.isChecked = prefs.getBoolean("split", false)

        fun updateSplitLabel() {
            val mode = prefs.getInt("split_mode", 0)
            lblSplitMode.text = if (mode == 0) "Bypass — всё через VPN, кроме списка" else "Proxy — только список через VPN"
        }
        updateSplitLabel()

        switchSplit.setOnCheckedChangeListener { _, isChecked ->
            prefs.edit().putBoolean("split", isChecked).apply()
        }

        btnSplitMode.setOnClickListener {
            val opts = arrayOf("Bypass — всё через VPN, кроме списка", "Proxy — только список через VPN")
            AlertDialog.Builder(requireContext(), R.style.DarkAlertDialog)
                .setTitle("Режим правил")
                .setSingleChoiceItems(opts, prefs.getInt("split_mode", 0)) { d, which ->
                    prefs.edit().putInt("split_mode", which).apply()
                    updateSplitLabel()
                    if (ForgeFoxVpnService.isRunning) {
                        requireContext().startService(android.content.Intent(requireContext(), ForgeFoxVpnService::class.java).apply { action = "START_VPN_SILENT" })
                    }
                    d.dismiss()
                }.show()
        }

        fun restartVpnIfNeeded() {
            if (ForgeFoxVpnService.isRunning) {
                val startIntent = android.content.Intent(requireContext(), ForgeFoxVpnService::class.java).apply {
                    action = "START_VPN_SILENT"
                }
                requireContext().startService(startIntent)
            }
        }

        // ===================== SITES =====================

        val btnSites = view.findViewById<View>(R.id.btnSites)
        val lblSites = view.findViewById<TextView>(R.id.lblSites)

        fun updateSitesLabel() {
            val n = SplitTunnel.siteEntries(prefs).size
            lblSites.text = if (n == 0) "Не задано" else "$n правил"
        }
        updateSitesLabel()

        btnSites.setOnClickListener {
            val pad = { dp: Int ->
                (dp * resources.displayMetrics.density).toInt()
            }
            val input = android.widget.EditText(requireContext()).apply {
                setText(prefs.getString("bypass_sites", "") ?: "")
                hint = "youtube.com\ngoogle.com\n1.2.3.4\n10.0.0.0/24"
                minLines = 6
                gravity = android.view.Gravity.TOP
                setTextColor(android.graphics.Color.WHITE)
                setHintTextColor(0xFF71717A.toInt())
                setBackgroundColor(0xFF18181B.toInt())
                setPadding(pad(16), pad(12), pad(16), pad(12))
            }
            AlertDialog.Builder(requireContext(), R.style.DarkAlertDialog)
                .setTitle("Сайты для правил")
                .setMessage("Домены, IP или CIDR — каждый с новой строки. Домены резолвятся при подключении.")
                .setView(input)
                .setPositiveButton("Сохранить") { _, _ ->
                    prefs.edit().putString("bypass_sites", input.text.toString()).apply()
                    updateSitesLabel()
                    restartVpnIfNeeded()
                }
                .setNegativeButton("Отмена", null)
                .show()
        }

        // ===================== APPS =====================

        val rvApps = view.findViewById<RecyclerView>(R.id.rvApps)
        rvApps.layoutManager = LinearLayoutManager(requireContext())

        val bypassedStr = prefs.getString("bypass_apps", "") ?: ""
        val bypassedSet = bypassedStr.split(",").map { it.trim() }.filter { it.isNotEmpty() }.toMutableSet()

        val appContext = requireContext().applicationContext
        Thread {
            try {
                val pm = appContext.packageManager
                // Pre-installed apps (Chrome, YouTube, Play Store…) carry
                // FLAG_SYSTEM, so filtering on it hides them. Instead show every
                // launchable app plus all user-installed apps.
                val launcher = android.content.Intent(android.content.Intent.ACTION_MAIN)
                    .addCategory(android.content.Intent.CATEGORY_LAUNCHER)
                val launchable = pm.queryIntentActivities(launcher, 0)
                    .map { it.activityInfo.packageName }
                    .toSet()
                val packages = pm.getInstalledApplications(PackageManager.GET_META_DATA)
                val appList = packages
                    .filter {
                        launchable.contains(it.packageName) ||
                            (it.flags and ApplicationInfo.FLAG_SYSTEM) == 0
                    }
                    .distinctBy { it.packageName }
                    .map { Triple(pm.getApplicationLabel(it).toString(), it.packageName, pm.getApplicationIcon(it)) }
                    .sortedBy { it.first.lowercase() }

                activity?.runOnUiThread {
                    if (isAdded) {
                        rvApps.adapter = AppToggleAdapter(appList, bypassedSet) {
                            prefs.edit().putString("bypass_apps", bypassedSet.joinToString(",")).apply()
                            restartVpnIfNeeded()
                        }
                    }
                }
            } catch (e: Exception) {
                ForgeFoxVpnService.addLog("Apps load error: ${e.message}")
            }
        }.start()

        return view
    }

    // ===================== ADAPTERS =====================

    inner class AppToggleAdapter(
        private val items: List<Triple<String, String, Drawable>>,
        private val bypassedSet: MutableSet<String>,
        private val onChanged: () -> Unit
    ) : RecyclerView.Adapter<AppToggleAdapter.VH>() {

        inner class VH(v: View) : RecyclerView.ViewHolder(v) {
            val icon: ImageView = v.findViewById(R.id.imgAppIcon)
            val name: TextView = v.findViewById(R.id.tvAppName)
            val pkg: TextView = v.findViewById(R.id.tvAppPackage)
            val sw: SwitchCompat = v.findViewById(R.id.switchApp)
        }

        override fun onCreateViewHolder(parent: ViewGroup, viewType: Int) =
            VH(LayoutInflater.from(parent.context).inflate(R.layout.item_app, parent, false))

        override fun onBindViewHolder(holder: VH, position: Int) {
            val (appName, pkgName, icon) = items[position]
            holder.icon.setImageDrawable(icon)
            holder.name.text = appName
            holder.pkg.text = pkgName
            holder.sw.setOnCheckedChangeListener(null)
            holder.sw.isChecked = bypassedSet.contains(pkgName)
            holder.sw.setOnCheckedChangeListener { _, checked ->
                if (checked) bypassedSet.add(pkgName) else bypassedSet.remove(pkgName)
                onChanged()
            }
        }

        override fun getItemCount() = items.size
    }
}
