package com.forgefox.vpn

import android.app.AlertDialog
import android.content.Intent
import android.content.SharedPreferences
import android.content.pm.ApplicationInfo
import android.content.pm.PackageManager
import android.graphics.Color
import android.graphics.drawable.Drawable
import android.os.Bundle
import android.text.Editable
import android.text.TextWatcher
import android.view.LayoutInflater
import android.view.View
import android.view.ViewGroup
import android.widget.EditText
import android.widget.ImageView
import android.widget.LinearLayout
import android.widget.TextView
import android.widget.Toast
import androidx.appcompat.widget.SwitchCompat
import androidx.fragment.app.Fragment
import androidx.recyclerview.widget.LinearLayoutManager
import androidx.recyclerview.widget.RecyclerView
import java.util.UUID

/**
 * Routing settings: split-tunnel toggle, mode, and the rule list —
 * the same management surface as the desktop's "Split" page: typed rules
 * (ip / cidr / domain / domain_zone / app), per-rule enable, filters,
 * add dialog with an app picker, and import/export in the desktop
 * plain-text format (`type:value` per line, `#` comments).
 */
class SettingsFragment : Fragment() {

    private lateinit var prefs: SharedPreferences
    private var rules = mutableListOf<SplitRules.Rule>()
    private var filter: SplitRules.Type? = null
    private var query = ""
    private var rulesAdapter: RulesAdapter? = null

    // Live list of launchable apps for the app-rule picker.
    private data class AppEntry(val label: String, val pkg: String, val icon: Drawable)
    private var installedApps: List<AppEntry> = emptyList()

    override fun onCreateView(
        inflater: LayoutInflater, container: ViewGroup?,
        savedInstanceState: Bundle?
    ): View? {
        val view = inflater.inflate(R.layout.fragment_settings, container, false)
        prefs = requireContext().getSharedPreferences("ForgeFoxSettings", 0)
        rules = SplitRules.load(prefs)

        // --- Split toggle ---
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
            restartVpnIfNeeded()
        }

        btnSplitMode.setOnClickListener {
            val opts = arrayOf("Bypass — всё через VPN, кроме списка", "Proxy — только список через VPN")
            AlertDialog.Builder(requireContext(), R.style.DarkAlertDialog)
                .setTitle("Режим правил")
                .setSingleChoiceItems(opts, prefs.getInt("split_mode", 0)) { d, which ->
                    prefs.edit().putInt("split_mode", which).apply()
                    updateSplitLabel()
                    restartVpnIfNeeded()
                    d.dismiss()
                }.show()
        }

        // --- Rules list ---
        val rvRules = view.findViewById<RecyclerView>(R.id.rvRules)
        rvRules.layoutManager = LinearLayoutManager(requireContext())
        rulesAdapter = RulesAdapter()
        rvRules.adapter = rulesAdapter

        val btnAddRule = view.findViewById<View>(R.id.btnAddRule)
        btnAddRule.setOnClickListener { showAddRuleDialog() }

        val btnExport = view.findViewById<View>(R.id.btnExport)
        btnExport.setOnClickListener { showIoDialog(exportMode = true) }

        val btnImport = view.findViewById<View>(R.id.btnImport)
        btnImport.setOnClickListener { showIoDialog(exportMode = false) }

        // --- Filters ---
        val chips = mapOf(
            R.id.chipAll to null,
            R.id.chipIp to SplitRules.Type.IP,
            R.id.chipCidr to SplitRules.Type.CIDR,
            R.id.chipDomain to SplitRules.Type.DOMAIN,
            R.id.chipZone to SplitRules.Type.DOMAIN_ZONE,
            R.id.chipApp to SplitRules.Type.APP
        )
        for ((id, type) in chips) {
            view.findViewById<TextView>(id).setOnClickListener {
                filter = type
                for ((otherId, _) in chips) {
                    view.findViewById<TextView>(otherId).apply {
                        setTextColor(if (otherId == id) Color.WHITE else 0xFFA1A1AA.toInt())
                    }
                }
                refreshRules()
            }
        }

        loadInstalledApps()

        return view
    }

    private fun restartVpnIfNeeded() {
        if (ForgeFoxVpnService.isRunning) {
            requireContext().startService(
                Intent(requireContext(), ForgeFoxVpnService::class.java).apply { action = "START_VPN_SILENT" }
            )
        }
    }

    private fun refreshRules() {
        rules = SplitRules.load(prefs)
        rulesAdapter?.notifyDataSetChanged()
        view?.findViewById<TextView>(R.id.lblNoRules)?.visibility =
            if (visibleRules().isEmpty()) View.VISIBLE else View.GONE
    }

    private fun visibleRules(): List<SplitRules.Rule> = rules.filter { r ->
        (filter == null || r.type == filter) &&
            (query.isEmpty() || r.normalized.contains(query, ignoreCase = true))
    }

    private fun persistRules() {
        SplitRules.save(prefs, rules)
        restartVpnIfNeeded()
    }

    private fun loadInstalledApps() {
        val appContext = requireContext().applicationContext
        Thread {
            try {
                val pm = appContext.packageManager
                val launcher = Intent(Intent.ACTION_MAIN).addCategory(Intent.CATEGORY_LAUNCHER)
                val launchable = pm.queryIntentActivities(launcher, 0)
                    .map { it.activityInfo.packageName }
                    .toSet()
                installedApps = pm.getInstalledApplications(PackageManager.GET_META_DATA)
                    .filter {
                        launchable.contains(it.packageName) ||
                            (it.flags and ApplicationInfo.FLAG_SYSTEM) == 0
                    }
                    .distinctBy { it.packageName }
                    .map { AppEntry(pm.getApplicationLabel(it).toString(), it.packageName, pm.getApplicationIcon(it)) }
                    .sortedBy { it.label.lowercase() }
            } catch (e: Exception) {
                ForgeFoxVpnService.addLog("Apps load error: ${e.message}")
            }
        }.start()
    }

    // ── dialogs ───────────────────────────────────────────────────────────────

    /** Add-rule dialog: type spinner + value field + optional app picker. */
    private fun showAddRuleDialog(selectedType: SplitRules.Type? = null, initialValue: String = "") {
        val pad = { dp: Int -> (dp * resources.displayMetrics.density).toInt() }
        val root = LinearLayout(requireContext())
        root.orientation = LinearLayout.VERTICAL
        root.setPadding(pad(20), pad(8), pad(20), pad(4))

        val typeSpinner = android.widget.Spinner(requireContext())
        val typeNames = SplitRules.Type.entries.map { it.label }.toTypedArray()
        val typeAdapter = android.widget.ArrayAdapter(requireContext(), android.R.layout.simple_spinner_item, typeNames)
        typeAdapter.setDropDownViewResource(android.R.layout.simple_spinner_dropdown_item)
        typeSpinner.adapter = typeAdapter
        typeSpinner.setSelection(selectedType?.ordinal ?: SplitRules.Type.DOMAIN_ZONE.ordinal)

        val input = EditText(requireContext()).apply {
            setText(initialValue)
            setTextColor(Color.WHITE)
            setHintTextColor(0xFF71717A.toInt())
            setBackgroundColor(0xFF18181B.toInt())
            setPadding(pad(12), pad(10), pad(12), pad(10))
            hint = "discord.com"
        }

        val lblHint = TextView(requireContext()).apply {
            setTextColor(0xFF71717A.toInt())
            textSize = 12f
            setPadding(0, pad(6), 0, 0)
        }

        fun updateHint() {
            val t = SplitRules.Type.entries[typeSpinner.selectedItemPosition]
            lblHint.text = when (t) {
                SplitRules.Type.DOMAIN -> "Только точное совпадение имени, без поддоменов"
                SplitRules.Type.DOMAIN_ZONE -> "Домен и все его поддомены (например discord.com + cdn.discord.com)"
                SplitRules.Type.APP -> "Имя пакета Android — можно выбрать из списка"
                SplitRules.Type.IP -> "Один адрес, например 78.17.19.94"
                SplitRules.Type.CIDR -> "Подсеть, например 10.0.0.0/8"
            }
            input.hint = t.hint
        }
        updateHint()

        val btnPickApp = TextView(requireContext()).apply {
            text = "⌨ Выбрать приложение…"
            setTextColor(0xFF71717A.toInt())
            textSize = 13f
            setPadding(0, pad(10), 0, pad(10))
            visibility = View.GONE
            setOnClickListener { showAppPicker { pkg -> input.setText(pkg) } }
        }
        root.addView(typeSpinner, LinearLayout.LayoutParams(LinearLayout.LayoutParams.MATCH_PARENT, LinearLayout.LayoutParams.WRAP_CONTENT))
        root.addView(input, LinearLayout.LayoutParams(LinearLayout.LayoutParams.MATCH_PARENT, LinearLayout.LayoutParams.WRAP_CONTENT).apply { topMargin = pad(12) })
        root.addView(lblHint)
        root.addView(btnPickApp)

        // Show the app picker link only for app rules
        fun syncAppLink() {
            btnPickApp.visibility =
                if (SplitRules.Type.entries[typeSpinner.selectedItemPosition] == SplitRules.Type.APP) View.VISIBLE else View.GONE
        }
        typeSpinner.post { syncAppLink() }
        typeSpinner.onItemSelectedListener = object : android.widget.AdapterView.OnItemSelectedListener {
            override fun onItemSelected(p: android.widget.AdapterView<*>, v: View?, pos: Int, id: Long) {
                updateHint(); syncAppLink()
            }
            override fun onNothingSelected(p: android.widget.AdapterView<*>) {}
        }
        AlertDialog.Builder(requireContext(), R.style.DarkAlertDialog)
            .setTitle("Добавить правило")
            .setView(root)
            .setPositiveButton("Добавить") { _, _ ->
                val type = SplitRules.Type.entries[typeSpinner.selectedItemPosition]
                val value = input.text.toString().trim()
                if (value.isEmpty()) {
                    Toast.makeText(context, "Введите значение", Toast.LENGTH_SHORT).show()
                    return@setPositiveButton
                }
                val rule = SplitRules.Rule(id = UUID.randomUUID().toString(), type = type, value = value, enabled = true)
                rule.validate()?.let {
                    Toast.makeText(context, it, Toast.LENGTH_LONG).show()
                    return@setPositiveButton
                }
                if (rules.any { it.type == rule.type && it.normalized == rule.normalized }) {
                    Toast.makeText(context, "Такое правило уже есть", Toast.LENGTH_SHORT).show()
                    return@setPositiveButton
                }
                rules.add(rule)
                persistRules()
                refreshRules()
            }
            .setNegativeButton("Отмена", null)
            .show()
    }

    /** App picker: search box + launchable app list. */
    private fun showAppPicker(onPick: (String) -> Unit) {
        val pad = { dp: Int -> (dp * resources.displayMetrics.density).toInt() }
        val search = EditText(requireContext()).apply {
            hint = "🔍 Поиск приложения…"
            setTextColor(Color.WHITE)
            setHintTextColor(0xFF71717A.toInt())
            setBackgroundColor(0xFF18181B.toInt())
            setPadding(pad(12), pad(10), pad(12), pad(10))
        }

        val list = RecyclerView(requireContext()).apply {
            layoutManager = LinearLayoutManager(requireContext())
        }
        val container = LinearLayout(requireContext()).apply {
            orientation = LinearLayout.VERTICAL
            addView(search, LinearLayout.LayoutParams(LinearLayout.LayoutParams.MATCH_PARENT, LinearLayout.LayoutParams.WRAP_CONTENT))
            addView(list, LinearLayout.LayoutParams(LinearLayout.LayoutParams.MATCH_PARENT, 900))
        }
        search.addTextChangedListener(object : TextWatcher {
            override fun afterTextChanged(s: Editable?) {
                (list.adapter as? AppPickerAdapter)?.filter(s?.toString() ?: "")
            }
            override fun beforeTextChanged(s: CharSequence?, a: Int, b: Int, c: Int) {}
            override fun onTextChanged(s: CharSequence?, a: Int, b: Int, c: Int) {}
        })

        var dialog: AlertDialog? = null
        val adapter = AppPickerAdapter(installedApps) { pkg ->
            onPick(pkg)
            dialog?.dismiss()
        }
        list.adapter = adapter

        if (installedApps.isEmpty()) {
            Toast.makeText(context, "Список приложений ещё загружается, попробуйте ещё раз", Toast.LENGTH_SHORT).show()
        }

        dialog = AlertDialog.Builder(requireContext(), R.style.DarkAlertDialog)
            .setTitle("Выбрать приложение")
            .setView(container)
            .setNegativeButton("Отмена", null)
            .show()
    }

    /**
     * Import / export dialog — same shape as the desktop's modal-io:
     * a textarea with the plain-text rule list, plus copy/paste buttons.
     */
    private fun showIoDialog(exportMode: Boolean) {
        val pad = { dp: Int -> (dp * resources.displayMetrics.density).toInt() }
        val input = EditText(requireContext()).apply {
            setTextColor(Color.WHITE)
            setHintTextColor(0xFF71717A.toInt())
            setBackgroundColor(0xFF18181B.toInt())
            setPadding(pad(16), pad(12), pad(16), pad(12))
            minLines = 10
            gravity = android.view.Gravity.TOP
            setText(SplitRules.export(rules))
            setTextIsSelectable(true)
            hint = "domain_zone:example.com\ncidr:10.0.0.0/8\napp:com.example.app"
        }

        val replaceCheck = android.widget.CheckBox(requireContext()).apply {
            text = "Заменить существующие правила"
            setTextColor(Color.WHITE)
            visibility = if (exportMode) View.GONE else View.VISIBLE
        }

        val container = LinearLayout(requireContext()).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(pad(20), pad(8), pad(20), pad(8))
            addView(input, LinearLayout.LayoutParams(LinearLayout.LayoutParams.MATCH_PARENT, 0, 1f))
            addView(replaceCheck, LinearLayout.LayoutParams(LinearLayout.LayoutParams.MATCH_PARENT, LinearLayout.LayoutParams.WRAP_CONTENT).apply { topMargin = pad(12) })
        }

        val dlg = AlertDialog.Builder(requireContext(), R.style.DarkAlertDialog)
            .setTitle(if (exportMode) "Экспорт правил" else "Импорт правил")
            .setMessage(if (exportMode) "Скопируйте список и вставьте его на ПК или другом устройстве." else "Вставьте список правил (формат экспорта ПК). Пути Windows (.exe) пропускаются.")
            .setView(container)
            .setPositiveButton(if (exportMode) "Скопировать" else "Импортировать") { d, _ ->
                if (exportMode) {
                    val cm = requireContext().getSystemService(android.content.ClipboardManager::class.java)
                    cm.setPrimaryClip(android.content.ClipData.newPlainText("ForgeFox rules", input.text.toString()))
                    Toast.makeText(context, "Скопировано в буфер обмена", Toast.LENGTH_SHORT).show()
                } else {
                    val (parsed, errors) = SplitRules.parseImport(input.text.toString())
                    if (parsed.isEmpty()) {
                        Toast.makeText(context, "Не найдено ни одного правила", Toast.LENGTH_SHORT).show()
                    } else {
                        val added = SplitRules.import(prefs, parsed, replaceCheck.isChecked)
                        Toast.makeText(context, "Импортировано правил: $added", Toast.LENGTH_SHORT).show()
                        if (errors.isNotEmpty()) {
                            ForgeFoxVpnService.addLog("Import warnings:\n" + errors.joinToString("\n"))
                            Toast.makeText(context, "${errors.size} строк пропущено (см. логи)", Toast.LENGTH_LONG).show()
                        }
                        restartVpnIfNeeded()
                        refreshRules()
                    }
                }
            }
            .setNeutralButton("Вставить из буфера", null)
            .setNegativeButton("Закрыть", null)
            .create()

        // Neutral button handled manually so the dialog stays open on paste.
        dlg.setOnShowListener {
            dlg.getButton(AlertDialog.BUTTON_NEUTRAL).setOnClickListener {
                val cm = requireContext().getSystemService(android.content.ClipboardManager::class.java)
                val text = cm.primaryClip?.getItemAt(0)?.text?.toString() ?: ""
                if (text.isNotBlank()) input.setText(text)
                else Toast.makeText(context, "Буфер обмена пуст", Toast.LENGTH_SHORT).show()
            }
        }
        dlg.show()
    }

    // ── rules adapter ─────────────────────────────────────────────────────────

    private inner class RulesAdapter : RecyclerView.Adapter<RulesAdapter.VH>() {

        inner class VH(v: View) : RecyclerView.ViewHolder(v) {
            val type: TextView = v.findViewById(R.id.tvRuleType)
            val value: TextView = v.findViewById(R.id.tvRuleValue)
            val hint: TextView = v.findViewById(R.id.tvRuleHint)
            val sw: SwitchCompat = v.findViewById(R.id.switchRule)
            val del: ImageView = v.findViewById(R.id.btnDeleteRule)
        }

        override fun onCreateViewHolder(parent: ViewGroup, viewType: Int) =
            VH(LayoutInflater.from(parent.context).inflate(R.layout.item_rule, parent, false))

        override fun getItemCount() = visibleRules().size

        override fun onBindViewHolder(holder: VH, position: Int) {
            val rule = visibleRules()[position]
            holder.type.text = when (rule.type) {
                SplitRules.Type.IP -> "IP"
                SplitRules.Type.CIDR -> "ПОДСЕТЬ"
                SplitRules.Type.DOMAIN -> "ДОМЕН"
                SplitRules.Type.DOMAIN_ZONE -> "ЗОНА"
                SplitRules.Type.APP -> "ПРИЛОЖЕНИЕ"
            }
            holder.value.text = rule.value
            holder.value.alpha = if (rule.enabled) 1f else 0.4f
            holder.hint.visibility = if (rule.type == SplitRules.Type.DOMAIN_ZONE) View.VISIBLE else View.GONE

            holder.sw.setOnCheckedChangeListener(null)
            holder.sw.isChecked = rule.enabled
            holder.sw.setOnCheckedChangeListener { _, checked ->
                val idx = rules.indexOfFirst { it.id == rule.id }
                if (idx >= 0) {
                    rules[idx] = rules[idx].copy(enabled = checked)
                    persistRules()
                    refreshRules()
                }
            }

            holder.del.setOnClickListener {
                val idx = rules.indexOfFirst { it.id == rule.id }
                if (idx >= 0) {
                    rules.removeAt(idx)
                    persistRules()
                    refreshRules()
                }
            }
        }
    }

    // ── app picker adapter ────────────────────────────────────────────────────

    private inner class AppPickerAdapter(
        items: List<AppEntry>,
        private val onPick: (String) -> Unit
    ) : RecyclerView.Adapter<AppPickerAdapter.VH>() {

        private var shown = items

        fun filter(q: String) {
            shown = installedApps.filter {
                it.label.contains(q, true) || it.pkg.contains(q, true)
            }
            notifyDataSetChanged()
        }

        inner class VH(v: View) : RecyclerView.ViewHolder(v) {
            val icon: ImageView = v.findViewById(R.id.imgAppIcon)
            val name: TextView = v.findViewById(R.id.tvAppName)
            val pkg: TextView = v.findViewById(R.id.tvAppPackage)
        }

        override fun onCreateViewHolder(parent: ViewGroup, viewType: Int) =
            VH(LayoutInflater.from(parent.context).inflate(R.layout.item_app, parent, false))

        override fun getItemCount() = shown.size

        override fun onBindViewHolder(holder: VH, position: Int) {
            val app = shown[position]
            holder.icon.setImageDrawable(app.icon)
            holder.name.text = app.label
            holder.pkg.text = app.pkg
            holder.itemView.setOnClickListener { onPick(app.pkg) }
        }
    }
}
