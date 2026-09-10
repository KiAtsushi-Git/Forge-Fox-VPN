package com.forgefox.vpn

import android.app.AlertDialog
import android.content.Context
import android.content.pm.ApplicationInfo
import android.content.pm.PackageManager
import android.graphics.drawable.Drawable
import android.net.Uri
import android.os.Bundle
import android.text.Editable
import android.text.TextWatcher
import android.view.LayoutInflater
import android.view.View
import android.view.ViewGroup
import android.widget.EditText
import android.widget.ImageView
import android.widget.TextView
import android.widget.Toast
import androidx.appcompat.widget.SwitchCompat
import androidx.fragment.app.Fragment
import androidx.recyclerview.widget.LinearLayoutManager
import androidx.recyclerview.widget.RecyclerView
import org.json.JSONArray
import org.json.JSONObject

class SettingsFragment : Fragment() {

    private data class AppEntry(val name: String, val pkg: String, val icon: Drawable)

    private var allApps: List<AppEntry> = emptyList()
    private var filteredApps: List<AppEntry> = emptyList()
    private val bypassedSet = mutableSetOf<String>()
    private var appsAdapter: AppToggleAdapter? = null
    private var currentQuery = ""

    // SAF pickers for rules export / import
    private val exportPicker =
        registerForActivityResult(androidx.activity.result.contract.ActivityResultContracts.CreateDocument("application/json")) { uri ->
            if (uri != null) exportRules(uri, asText = false)
        }
    private val exportPickerTxt =
        registerForActivityResult(androidx.activity.result.contract.ActivityResultContracts.CreateDocument("text/plain")) { uri ->
            if (uri != null) exportRules(uri, asText = true)
        }
    private val importPicker =
        registerForActivityResult(androidx.activity.result.contract.ActivityResultContracts.OpenDocument()) { uri ->
            if (uri != null) importRules(uri)
        }

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
            val input = EditText(requireContext()).apply {
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
                .setMessage(
                    "Домены, IP или CIDR — каждый с новой строки. Домены резолвятся при подключении.\n\n" +
                    "Зоны вида *.ru или *.io — целая доменная зона через VPN (работает в режиме Proxy)."
                )
                .setView(input)
                .setPositiveButton("Сохранить") { _, _ ->
                    val raw = input.text.toString()
                    val entries = SplitTunnel.siteEntriesFromRaw(raw)
                    val invalid = SplitTunnel.invalidEntries(entries)
                    if (invalid.isNotEmpty()) {
                        Toast.makeText(
                            requireContext(),
                            "Пропущено (не поддерживается): ${invalid.joinToString(", ")}",
                            Toast.LENGTH_LONG
                        ).show()
                    }
                    val hasZones = entries.any { it.startsWith("*.") }
                    if (hasZones && prefs.getInt("split_mode", 0) != 1) {
                        Toast.makeText(
                            requireContext(),
                            "Зоны (*.ru) работают только в режиме Proxy — в Bypass они будут игнорироваться",
                            Toast.LENGTH_LONG
                        ).show()
                    }
                    prefs.edit().putString("bypass_sites", entries.joinToString("\n")).apply()
                    updateSitesLabel()
                    restartVpnIfNeeded()
                }
                .setNegativeButton("Отмена", null)
                .show()
        }

        // ===================== EXPORT / IMPORT =====================

        view.findViewById<View>(R.id.btnExportRules).setOnClickListener {
            AlertDialog.Builder(requireContext(), R.style.DarkAlertDialog)
                .setTitle("Формат экспорта")
                .setMessage(
                    "JSON — для переноса между телефонами.\n" +
                    "TXT — формат ПК-клиента (domain_zone:…), для обмена с десктопом."
                )
                .setPositiveButton("JSON") { _, _ -> exportPicker.launch("forgefox-rules.json") }
                .setNegativeButton("TXT (ПК)") { _, _ -> exportPickerTxt.launch("forgefox-rules.txt") }
                .show()
        }
        view.findViewById<View>(R.id.btnImportRules).setOnClickListener {
            importPicker.launch(arrayOf("application/json", "application/octet-stream", "text/plain", "*/*"))
        }

        // ===================== APPS =====================

        val rvApps = view.findViewById<RecyclerView>(R.id.rvApps)
        rvApps.layoutManager = LinearLayoutManager(requireContext())
        appsAdapter = AppToggleAdapter(
            onClick = { pkg -> toggleApp(pkg, prefs, ::restartVpnIfNeeded) }
        )
        rvApps.adapter = appsAdapter

        bypassedSet.clear()
        bypassedSet.addAll(
            (prefs.getString("bypass_apps", "") ?: "")
                .split(",").map { it.trim() }.filter { it.isNotEmpty() }
        )

        view.findViewById<EditText>(R.id.etAppSearch).addTextChangedListener(object : TextWatcher {
            override fun beforeTextChanged(s: CharSequence?, a: Int, b: Int, c: Int) {}
            override fun onTextChanged(s: CharSequence?, a: Int, b: Int, c: Int) {}
            override fun afterTextChanged(s: Editable?) {
                currentQuery = s?.toString()?.trim() ?: ""
                applyFilter()
            }
        })

        view.findViewById<View>(R.id.btnSelectAllApps).setOnClickListener {
            for (app in filteredApps) bypassedSet.add(app.pkg)
            persistApps(prefs)
            appsAdapter?.notifyAppChanges()
            restartVpnIfNeeded()
        }
        view.findViewById<View>(R.id.btnSelectNoneApps).setOnClickListener {
            for (app in filteredApps) bypassedSet.remove(app.pkg)
            persistApps(prefs)
            appsAdapter?.notifyAppChanges()
            restartVpnIfNeeded()
        }

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
                    .map { AppEntry(pm.getApplicationLabel(it).toString(), it.packageName, pm.getApplicationIcon(it)) }
                    .sortedBy { it.name.lowercase() }

                activity?.runOnUiThread {
                    if (isAdded) {
                        allApps = appList
                        applyFilter()
                    }
                }
            } catch (e: Exception) {
                ForgeFoxVpnService.addLog("Apps load error: ${e.message}")
            }
        }.start()

        return view
    }

    // ── app list helpers ───────────────────────────────────────────────────

    private fun applyFilter() {
        filteredApps = if (currentQuery.isEmpty()) allApps
        else allApps.filter {
            it.name.contains(currentQuery, ignoreCase = true) ||
                it.pkg.contains(currentQuery, ignoreCase = true)
        }
        appsAdapter?.notifyAppChanges()
    }

    private fun toggleApp(pkg: String, prefs: android.content.SharedPreferences, restart: () -> Unit) {
        if (!bypassedSet.remove(pkg)) bypassedSet.add(pkg)
        persistApps(prefs)
        appsAdapter?.notifyAppChanges()
        restart()
    }

    private fun persistApps(prefs: android.content.SharedPreferences) {
        prefs.edit().putString("bypass_apps", bypassedSet.joinToString(",")).apply()
    }

    // ── export / import ────────────────────────────────────────────────────

    private fun buildRulesJson(): JSONObject {
        val prefs = requireContext().getSharedPreferences("ForgeFoxSettings", 0)
        val sitesArr = JSONArray()
        SplitTunnel.siteEntries(prefs).forEach { sitesArr.put(it) }
        val appsArr = JSONArray()
        bypassedSet.sorted().forEach { appsArr.put(it) }
        return JSONObject().apply {
            put("version", 1)
            put("app", "ForgeFoxVPN")
            put("split_enabled", prefs.getBoolean("split", false))
            put("split_mode", prefs.getInt("split_mode", 0))
            put("sites", sitesArr)
            put("apps", appsArr)
        }
    }

    /** PC-client text format — same shape the desktop app exports. */
    private fun buildRulesText(): String {
        val prefs = requireContext().getSharedPreferences("ForgeFoxSettings", 0)
        val sb = StringBuilder("# ForgeFoxVPN split-tunnel rules\n# формат: тип:значение\n")
        for (entry in SplitTunnel.siteEntries(prefs)) {
            val parts = entry.removePrefix("*.").split(".")
            val type = when {
                entry.startsWith("*.") -> "domain_zone"
                entry.contains('/') -> "cidr"
                parts.size == 4 && parts.all { it.toIntOrNull() in 0..255 } -> "ip"
                else -> "domain"
            }
            sb.append("$type:").append(entry.removePrefix("*.")).append('\n')
        }
        for (pkg in bypassedSet.sorted()) sb.append("app:").append(pkg).append('\n')
        return sb.toString()
    }

    private fun exportRules(uri: Uri, asText: Boolean) {
        try {
            val data = if (asText) buildRulesText() else buildRulesJson().toString(2)
            requireContext().contentResolver.openOutputStream(uri, "wt")?.use { out ->
                out.write(data.toByteArray(Charsets.UTF_8))
            } ?: throw IllegalStateException("cannot open output stream")
            Toast.makeText(requireContext(), "Правила экспортированы", Toast.LENGTH_SHORT).show()
        } catch (e: Exception) {
            Toast.makeText(requireContext(), "Ошибка экспорта: ${e.message}", Toast.LENGTH_LONG).show()
        }
    }

    /**
     * Parse the PC-client text format ("тип:значение" per line, "#"
     * comments). Windows .exe paths in app rules are skipped — they can
     * never match an Android package.
     * Returns (sites, apps, per-line errors).
     */
    private fun parseRulesText(text: String): Triple<List<String>, List<String>, List<String>> {
        val sites = mutableListOf<String>()
        val apps = mutableListOf<String>()
        val errors = mutableListOf<String>()
        for ((idx, raw) in text.lines().withIndex()) {
            val line = raw.trim()
            if (line.isEmpty() || line.startsWith("#")) continue
            val colon = line.indexOf(':')
            if (colon <= 0) {
                errors.add("Строка ${idx + 1}: не разобрать «$line»")
                continue
            }
            val type = line.substring(0, colon).lowercase()
            val value = line.substring(colon + 1).trim()
            if (value.isEmpty()) {
                errors.add("Строка ${idx + 1}: пустое значение")
                continue
            }
            when (type) {
                "ip" -> sites.add(value)
                "cidr", "subnet", "net" -> sites.add(value)
                "domain", "site" -> sites.add(value)
                "domain_zone", "domainzone", "zone" -> sites.add("*.$value")
                "app", "exe", "process", "package" ->
                    // A Windows path from the PC export — meaningless on Android.
                    if (!value.contains('\\') && !value.contains('/')) apps.add(value)
                else -> errors.add("Строка ${idx + 1}: неизвестный тип «$type»")
            }
        }
        return Triple(sites, apps, errors)
    }

    private fun importRules(uri: Uri) {
        val prefs = requireContext().getSharedPreferences("ForgeFoxSettings", 0)
        try {
            val text = requireContext().contentResolver.openInputStream(uri)?.use { inp ->
                inp.bufferedReader(Charsets.UTF_8).readText()
            } ?: throw IllegalStateException("cannot open input stream")

            // Auto-detect: JSON starts with '{', everything else is treated as
            // the PC-client text format (works for .txt / .conf / any text).
            val isJson = text.trimStart().startsWith("{")
            var fileSites: List<String> = emptyList()
            var fileApps: Set<String> = emptySet()
            var parseErrors: List<String> = emptyList()
            var jsonSplitEnabled: Boolean? = null
            var jsonSplitMode: Int? = null

            if (isJson) {
                val root = JSONObject(text)
                val sites = mutableListOf<String>()
                val sitesArr = root.optJSONArray("sites")
                if (sitesArr != null) {
                    for (i in 0 until sitesArr.length()) {
                        val s = sitesArr.optString(i).trim()
                        if (s.isNotEmpty()) sites.add(s)
                    }
                }
                val apps = mutableSetOf<String>()
                val appsArr = root.optJSONArray("apps")
                if (appsArr != null) {
                    for (i in 0 until appsArr.length()) {
                        val s = appsArr.optString(i).trim()
                        if (s.isNotEmpty()) apps.add(s)
                    }
                }
                fileSites = sites
                fileApps = apps
                if (root.has("split_enabled")) jsonSplitEnabled = root.getBoolean("split_enabled")
                if (root.has("split_mode")) jsonSplitMode = root.getInt("split_mode")
            } else {
                val (sites, apps, errors) = parseRulesText(text)
                fileSites = sites
                fileApps = apps.toSet()
                parseErrors = errors
            }

            if (parseErrors.isNotEmpty()) {
                Toast.makeText(
                    requireContext(),
                    "Пропущено: ${parseErrors.take(3).joinToString("; ")}" +
                        if (parseErrors.size > 3) " (+${parseErrors.size - 3})" else "",
                    Toast.LENGTH_LONG
                ).show()
            }
            if (fileSites.isEmpty() && fileApps.isEmpty()) {
                Toast.makeText(requireContext(), "В файле нет правил (сайтов и приложений)", Toast.LENGTH_LONG).show()
                return
            }

            AlertDialog.Builder(requireContext(), R.style.DarkAlertDialog)
                .setTitle("Импорт правил")
                .setMessage(
                    "В файле: ${fileSites.size} сайтов, ${fileApps.size} приложений.\n\n" +
                    "Заменить — текущие правила будут перезаписаны.\n" +
                    "Объединить — списки будут дополнены."
                )
                .setPositiveButton("Заменить") { _, _ ->
                    val invalid = SplitTunnel.invalidEntries(fileSites)
                    if (invalid.isNotEmpty()) {
                        Toast.makeText(
                            requireContext(),
                            "Пропущено (не поддерживается): ${invalid.joinToString(", ")}",
                            Toast.LENGTH_LONG
                        ).show()
                    }
                    prefs.edit()
                        .putString("bypass_sites", fileSites.joinToString("\n"))
                        .putString("bypass_apps", fileApps.joinToString(","))
                        .apply()
                    jsonSplitEnabled?.let { prefs.edit().putBoolean("split", it).apply() }
                    jsonSplitMode?.let { prefs.edit().putInt("split_mode", it).apply() }
                    afterImport(prefs)
                }
                .setNeutralButton("Объединить") { _, _ ->
                    val mergedSites = (SplitTunnel.siteEntries(prefs) + fileSites).distinct()
                    val mergedApps = bypassedSet + fileApps
                    val invalid = SplitTunnel.invalidEntries(mergedSites)
                    if (invalid.isNotEmpty()) {
                        Toast.makeText(
                            requireContext(),
                            "Пропущено (не поддерживается): ${invalid.joinToString(", ")}",
                            Toast.LENGTH_LONG
                        ).show()
                    }
                    prefs.edit()
                        .putString("bypass_sites", mergedSites.joinToString("\n"))
                        .putString("bypass_apps", mergedApps.joinToString(","))
                        .apply()
                    afterImport(prefs)
                }
                .setNegativeButton("Отмена", null)
                .show()
        } catch (e: Exception) {
            Toast.makeText(requireContext(), "Ошибка импорта: ${e.message}", Toast.LENGTH_LONG).show()
        }
    }

    private fun afterImport(prefs: android.content.SharedPreferences) {
        bypassedSet.clear()
        bypassedSet.addAll(
            (prefs.getString("bypass_apps", "") ?: "")
                .split(",").map { it.trim() }.filter { it.isNotEmpty() }
        )
        view?.findViewById<TextView>(R.id.lblSites)?.text =
            SplitTunnel.siteEntries(prefs).size.let { n -> if (n == 0) "Не задано" else "$n правил" }
        appsAdapter?.notifyAppChanges()
        if (ForgeFoxVpnService.isRunning) {
            requireContext().startService(
                android.content.Intent(requireContext(), ForgeFoxVpnService::class.java).apply {
                    action = "START_VPN_SILENT"
                }
            )
        }
    }

    // ===================== ADAPTERS =====================

    /**
     * Multi-select list: tapping a row toggles the app, the switch mirrors
     * the state. "Выбрать все / Снять все" operate on the filtered list.
     */
    inner class AppToggleAdapter(
        private val onClick: (String) -> Unit
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
            val app = filteredApps[position]
            holder.icon.setImageDrawable(app.icon)
            holder.name.text = app.name
            holder.pkg.text = app.pkg
            holder.sw.setOnCheckedChangeListener(null)
            holder.sw.isChecked = bypassedSet.contains(app.pkg)
            fun toggle() = onClick(app.pkg)
            holder.sw.setOnCheckedChangeListener { _, _ -> toggle() }
            holder.itemView.setOnClickListener { toggle() }
        }

        override fun getItemCount() = filteredApps.size

        fun notifyAppChanges() = notifyDataSetChanged()
    }
}
