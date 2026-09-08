package com.forgefox.vpn

import android.app.AlertDialog
import android.content.Context
import android.os.Bundle
import android.view.LayoutInflater
import android.view.View
import android.view.ViewGroup
import android.widget.EditText
import android.widget.TextView
import android.widget.Toast
import androidx.fragment.app.Fragment
import androidx.recyclerview.widget.LinearLayoutManager
import androidx.recyclerview.widget.RecyclerView
import com.google.android.material.floatingactionbutton.FloatingActionButton
import org.json.JSONArray
import org.json.JSONObject
import java.net.HttpURLConnection
import java.net.URL
import kotlin.concurrent.thread

class ServersFragment : Fragment() {
    private lateinit var rvServers: RecyclerView
    private lateinit var adapter: ServerAdapter
    private val rawServersList = mutableListOf<JSONObject>()
    
    data class DisplayItem(
        val type: Int, // 0 = Single Node, 1 = Subscription Header, 2 = Subscription Child Node
        val name: String,
        val link: String,
        val obj: JSONObject? = null,
        var isExpanded: Boolean = false,
        val subNodes: JSONArray? = null,
        var pingText: String = "Ping",
        var isPinging: Boolean = false
    )
    
    private val displayList = mutableListOf<DisplayItem>()

    override fun onCreateView(
        inflater: LayoutInflater, container: ViewGroup?,
        savedInstanceState: Bundle?
    ): View? {
        val view = inflater.inflate(R.layout.fragment_servers, container, false)
        rvServers = view.findViewById(R.id.rvServers)
        rvServers.layoutManager = LinearLayoutManager(context)
        adapter = ServerAdapter()
        rvServers.adapter = adapter

        view.findViewById<FloatingActionButton>(R.id.fabAdd).setOnClickListener {
            showAddDialog()
        }

        view.findViewById<View>(R.id.btnPingAll).setOnClickListener { btn ->
            // Rotate animation
            val rotation = android.animation.ObjectAnimator.ofFloat(btn, "rotation", 0f, 360f)
            rotation.duration = 1000
            rotation.repeatCount = android.animation.ValueAnimator.INFINITE
            rotation.start()

            // Ping all nodes logic will go here
            thread {
                pingAllNodes()
                
                requireActivity().runOnUiThread {
                    rotation.cancel()
                    btn.rotation = 0f
                }
            }
        }

        loadServers()
        return view
    }

    private fun buildDisplayList() {
        displayList.clear()
        for (server in rawServersList) {
            if (server.optString("type") == "subscription") {
                val item = DisplayItem(
                    type = 1,
                    name = server.optString("name", "Подписка"),
                    link = server.optString("url", ""),
                    obj = server,
                    subNodes = server.optJSONArray("nodes")
                )
                // keep expansion state if it was expanded before (will be false by default)
                displayList.add(item)
            } else {
                displayList.add(DisplayItem(
                    type = 0,
                    name = server.optString("name", "Узел"),
                    link = server.optString("link", ""),
                    obj = server
                ))
            }
        }
        adapter.notifyDataSetChanged()
    }

    private fun loadServers() {
        val prefs = requireContext().getSharedPreferences("ForgeFoxServers", Context.MODE_PRIVATE)
        val data = prefs.getString("servers", "[]") ?: "[]"
        rawServersList.clear()
        try {
            val arr = JSONArray(data)
            for (i in 0 until arr.length()) {
                rawServersList.add(arr.getJSONObject(i))
            }
        } catch (e: Exception) {
            e.printStackTrace()
        }
        buildDisplayList()
    }

    private fun saveServers() {
        val prefs = requireContext().getSharedPreferences("ForgeFoxServers", Context.MODE_PRIVATE)
        val arr = JSONArray()
        rawServersList.forEach { arr.put(it) }
        prefs.edit().putString("servers", arr.toString()).apply()
    }

    private fun showAddDialog() {
        val editText = EditText(context)
        editText.hint = "ssh://... или http://..."
        
        AlertDialog.Builder(context)
            .setTitle("Добавить узел или подписку")
            .setView(editText)
            .setPositiveButton("Добавить") { _, _ ->
                val link = editText.text.toString().trim()
                if (link.startsWith("http")) {
                    fetchSubscription(link)
                } else if (link.startsWith("ssh://")) {
                    addVlessLink(link)
                } else {
                    Toast.makeText(context, "Неверный формат ссылки", Toast.LENGTH_SHORT).show()
                }
            }
            .setNegativeButton("Отмена", null)
            .show()
    }

    private fun fetchSubscription(urlStr: String) {
        activity?.runOnUiThread {
            context?.let { Toast.makeText(it, "Загрузка подписки...", Toast.LENGTH_SHORT).show() }
        }
        thread {
            try {
                var currentUrl = urlStr
                var redirects = 0
                var conn: HttpURLConnection? = null
                
                while (redirects < 5) {
                    val url = URL(currentUrl)
                    conn = url.openConnection() as HttpURLConnection
                    conn.instanceFollowRedirects = false
                    conn.requestMethod = "GET"
                    conn.setRequestProperty("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64)")
                    
                    val status = conn.responseCode
                    if (status == HttpURLConnection.HTTP_MOVED_TEMP || status == HttpURLConnection.HTTP_MOVED_PERM || status == HttpURLConnection.HTTP_SEE_OTHER) {
                        val newUrl = conn.getHeaderField("Location")
                        if (newUrl != null) {
                            currentUrl = newUrl
                            redirects++
                            continue
                        }
                    }
                    break
                }
                
                val response = conn?.inputStream?.bufferedReader()?.readText()?.trim() ?: ""
                
                if (response.isEmpty()) {
                    activity?.runOnUiThread {
                        context?.let { Toast.makeText(it, "Пустой ответ от сервера!", Toast.LENGTH_SHORT).show() }
                    }
                    return@thread
                }
                
                var decoded = response
                if (!decoded.contains("ssh://")) {
                    try {
                        val bytes = android.util.Base64.decode(response, android.util.Base64.DEFAULT)
                        decoded = String(bytes, Charsets.UTF_8)
                    } catch (e: Exception) {
                        e.printStackTrace()
                    }
                }
                
                activity?.runOnUiThread {
                    val parts = decoded.split('\n', '\r')
                    val nodesArr = JSONArray()
                    var count = 0
                    for (part in parts) {
                        val trimmed = part.trim()
                        if (trimmed.contains("ssh://")) {
                            val link = trimmed.substring(trimmed.indexOf("ssh://"))
                            
                            var name = "Узел-\${(1000..9999).random()}"
                            if (link.contains("#")) {
                                try {
                                    name = java.net.URLDecoder.decode(link.substringAfterLast("#"), "UTF-8")
                                } catch (e: Exception) {
                                    name = link.substringAfterLast("#")
                                }
                            }
                            val nodeObj = JSONObject()
                            nodeObj.put("name", name)
                            nodeObj.put("link", link)
                            nodesArr.put(nodeObj)
                            count++
                        }
                    }
                    
                    if (count > 0) {
                        val subName = urlStr.substringAfter("://").substringBefore("/")
                        val subObj = JSONObject()
                        subObj.put("type", "subscription")
                        subObj.put("name", "Подписка: $subName")
                        subObj.put("url", urlStr)
                        subObj.put("nodes", nodesArr)
                        
                        rawServersList.add(subObj)
                        saveServers()
                        buildDisplayList()
                        context?.let { Toast.makeText(it, "Добавлена подписка с $count узлами!", Toast.LENGTH_SHORT).show() }
                    } else {
                        context?.let { Toast.makeText(it, "Узлы не найдены в ответе сервера", Toast.LENGTH_LONG).show() }
                    }
                }
            } catch (e: Exception) {
                e.printStackTrace()
                activity?.runOnUiThread {
                    context?.let { Toast.makeText(it, "Ошибка: ${e.message}", Toast.LENGTH_LONG).show() }
                }
            }
        }
    }

    private fun addVlessLink(link: String, save: Boolean = true) {
        var name = "Узел-${(1000..9999).random()}"
        if (link.contains("#")) {
            try {
                name = java.net.URLDecoder.decode(link.substringAfterLast("#"), "UTF-8")
            } catch (e: Exception) {
                name = link.substringAfterLast("#")
            }
        }
        val obj = JSONObject()
        obj.put("type", "node")
        obj.put("name", name)
        obj.put("link", link)
        rawServersList.add(obj)
        if (save) {
            saveServers()
            buildDisplayList()
            Toast.makeText(context, "Узел добавлен", Toast.LENGTH_SHORT).show()
        }
    }

    inner class ServerAdapter : RecyclerView.Adapter<ServerAdapter.ViewHolder>() {
        inner class ViewHolder(view: View) : RecyclerView.ViewHolder(view) {
            val tvName: TextView = view.findViewById(R.id.tvName)
            val tvDesc: TextView = view.findViewById(R.id.tvDesc)
            val btnPing: View = view.findViewById(R.id.btnPing)
            val tvPing: TextView = view.findViewById(R.id.tvPing)
            val btnUpdate: View? = view.findViewById(R.id.btnUpdate)
            val btnCopyLink: View? = view.findViewById(R.id.btnCopyLink)
            val btnViewJson: View? = view.findViewById(R.id.btnViewJson)
            val cardContainer: View = view.findViewById(R.id.cardContainer)
        }

        override fun onCreateViewHolder(parent: ViewGroup, viewType: Int): ViewHolder {
            val view = LayoutInflater.from(parent.context).inflate(R.layout.item_server, parent, false)
            return ViewHolder(view)
        }

        override fun onBindViewHolder(holder: ViewHolder, position: Int) {
            val item = displayList[position]
            val prefs = requireContext().getSharedPreferences("ForgeFoxSettings", Context.MODE_PRIVATE)
            val selectedVless = prefs.getString("selected_vless", "")

            holder.tvName.text = item.name

            // Visual hierarchy and selection highlighting
            val dpScale = holder.itemView.context.resources.displayMetrics.density
            val defaultPadding = (16 * dpScale).toInt()

            if (item.type == 1) { // Subscription Header
                val count = item.subNodes?.length() ?: 0
                holder.tvDesc.text = if (item.isExpanded) "▲ Свернуть ($count)" else "▼ Развернуть ($count)"
                holder.btnPing.visibility = View.GONE
                holder.btnUpdate?.visibility = View.VISIBLE
                holder.cardContainer.setBackgroundResource(R.drawable.card_glass)
                holder.tvName.setTextColor(android.graphics.Color.WHITE)
            } else if (item.type == 2) { // Subscription Child
                holder.tvDesc.text = "Узел подписки"
                holder.btnPing.visibility = View.VISIBLE
                holder.btnUpdate?.visibility = View.GONE
                if (item.link == selectedVless) {
                    holder.cardContainer.setBackgroundResource(R.drawable.card_glass_active)
                    holder.tvName.setTextColor(android.graphics.Color.parseColor("#FF6B00"))
                } else {
                    holder.cardContainer.setBackgroundResource(R.drawable.card_glass)
                    holder.tvName.setTextColor(android.graphics.Color.parseColor("#D4D4D8"))
                }
            } else { // Single Node
                holder.tvDesc.text = "Одиночный узел"
                holder.btnPing.visibility = View.VISIBLE
                holder.btnUpdate?.visibility = View.GONE
                if (item.link == selectedVless) {
                    holder.cardContainer.setBackgroundResource(R.drawable.card_glass_active)
                    holder.tvName.setTextColor(android.graphics.Color.parseColor("#FF6B00"))
                } else {
                    holder.cardContainer.setBackgroundResource(R.drawable.card_glass)
                    holder.tvName.setTextColor(android.graphics.Color.WHITE)
                }
            }

            holder.tvPing.text = item.pingText
            if (item.isPinging) {
                holder.tvPing.setTextColor(android.graphics.Color.parseColor("#FF8C00")) // Orange
            } else if (item.pingText.endsWith("ms")) {
                val ms = item.pingText.removeSuffix("ms").toLongOrNull() ?: 0L
                if (ms < 150) {
                    holder.tvPing.setTextColor(android.graphics.Color.parseColor("#10B981")) // Green
                } else if (ms < 400) {
                    holder.tvPing.setTextColor(android.graphics.Color.parseColor("#FBBF24")) // Yellow
                } else {
                    holder.tvPing.setTextColor(android.graphics.Color.parseColor("#EF4444")) // Red
                }
            } else if (item.pingText == "Ping") {
                holder.tvPing.setTextColor(android.graphics.Color.parseColor("#A1A1AA")) // Gray
            } else {
                holder.tvPing.setTextColor(android.graphics.Color.parseColor("#DC2626")) // Bright Red for Fail/Error
            }

            holder.btnPing.setOnClickListener {
                if (!item.isPinging) {
                    pingNode(position)
                }
            }

            holder.btnUpdate?.setOnClickListener {
                if (item.type == 1 && item.link.isNotEmpty()) {
                    updateSubscription(position)
                }
            }

            holder.btnCopyLink?.visibility = if (item.type == 1) View.GONE else View.VISIBLE
            holder.btnViewJson?.visibility = if (item.type == 1) View.GONE else View.VISIBLE

            holder.btnCopyLink?.setOnClickListener {
                val clip = android.content.ClipData.newPlainText("vless link", item.link)
                (requireContext().getSystemService(android.content.Context.CLIPBOARD_SERVICE) as android.content.ClipboardManager).setPrimaryClip(clip)
                Toast.makeText(context, "Ссылка скопирована", Toast.LENGTH_SHORT).show()
            }

            holder.btnViewJson?.setOnClickListener {
                try {
                    val et = android.widget.EditText(requireContext())
                    et.setText(item.link)
                    et.setTextColor(android.graphics.Color.WHITE)
                    et.setBackgroundColor(android.graphics.Color.parseColor("#09090B"))
                    et.textSize = 10f
                    
                    val scroll = android.widget.ScrollView(requireContext())
                    scroll.addView(et)

                    android.app.AlertDialog.Builder(requireContext(), R.style.DarkAlertDialog)
                        .setTitle("JSON Конфиг")
                        .setView(scroll)
                        .setPositiveButton("Скопировать") { _, _ ->
                            val clip = android.content.ClipData.newPlainText("json config", item.link)
                            (requireContext().getSystemService(android.content.Context.CLIPBOARD_SERVICE) as android.content.ClipboardManager).setPrimaryClip(clip)
                            Toast.makeText(context, "JSON скопирован", Toast.LENGTH_SHORT).show()
                        }
                        .setNegativeButton("Закрыть", null)
                        .show()
                } catch (e: Exception) {
                    Toast.makeText(context, "Ошибка: ${e.message}", Toast.LENGTH_SHORT).show()
                }
            }

            holder.cardContainer.setOnClickListener {
                if (item.type == 1) {
                    toggleSubscription(position)
                } else {
                    selectNode(item.name, item.link)
                }
            }
        }

        private fun updateSubscription(position: Int) {
            val item = displayList[position]
            val urlStr = item.link
            if (urlStr.isEmpty()) return
            Toast.makeText(context, "Обновление подписки...", Toast.LENGTH_SHORT).show()
            thread {
                try {
                    var currentUrl = urlStr
                    var redirects = 0
                    var conn: HttpURLConnection? = null
                    
                    while (redirects < 5) {
                        val url = URL(currentUrl)
                        conn = url.openConnection() as HttpURLConnection
                        conn.instanceFollowRedirects = false
                        conn.requestMethod = "GET"
                        conn.setRequestProperty("User-Agent", "Mozilla/5.0")
                        val status = conn.responseCode
                        if (status == 301 || status == 302 || status == 303) {
                            val newUrl = conn.getHeaderField("Location")
                            if (newUrl != null) {
                                currentUrl = newUrl
                                redirects++
                                continue
                            }
                        }
                        break
                    }
                    val response = conn?.inputStream?.bufferedReader()?.readText()?.trim() ?: ""
                    var decoded = response
                    if (!decoded.contains("ssh://")) {
                        try {
                            val bytes = android.util.Base64.decode(response, android.util.Base64.DEFAULT)
                            decoded = String(bytes, Charsets.UTF_8)
                        } catch (e: Exception) {}
                    }
                    activity?.runOnUiThread {
                        val parts = decoded.split('\n', '\r')
                        val nodesArr = JSONArray()
                        for (part in parts) {
                            val trimmed = part.trim()
                            if (trimmed.contains("ssh://")) {
                                val link = trimmed.substring(trimmed.indexOf("ssh://"))
                                var name = "Узел"
                                if (link.contains("#")) {
                                    try { name = java.net.URLDecoder.decode(link.substringAfterLast("#"), "UTF-8") } 
                                    catch (e: Exception) { name = link.substringAfterLast("#") }
                                }
                                val nodeObj = JSONObject()
                                nodeObj.put("name", name)
                                nodeObj.put("link", link)
                                nodesArr.put(nodeObj)
                            }
                        }
                        if (nodesArr.length() > 0) {
                            for (i in 0 until rawServersList.size) {
                                val obj = rawServersList[i]
                                if (obj.optString("type") == "subscription" && obj.optString("url") == urlStr) {
                                    obj.put("nodes", nodesArr)
                                    break
                                }
                            }
                            saveServers()
                            buildDisplayList()
                            Toast.makeText(context, "Подписка обновлена! Узлов: ${nodesArr.length()}", Toast.LENGTH_SHORT).show()
                        } else {
                            Toast.makeText(context, "Не удалось обновить: узлы не найдены", Toast.LENGTH_SHORT).show()
                        }
                    }
                } catch (e: Exception) {
                    activity?.runOnUiThread {
                        Toast.makeText(context, "Ошибка обновления: ${e.message}", Toast.LENGTH_SHORT).show()
                    }
                }
            }
        }

        private fun toggleSubscription(position: Int) {
            val item = displayList[position]
            item.isExpanded = !item.isExpanded
            
            if (item.isExpanded) {
                val nodesArr = item.subNodes ?: JSONArray()
                val children = mutableListOf<DisplayItem>()
                for (i in 0 until nodesArr.length()) {
                    val node = nodesArr.getJSONObject(i)
                    children.add(DisplayItem(
                        type = 2,
                        name = node.optString("name", "Узел"),
                        link = node.optString("link", "")
                    ))
                }
                displayList.addAll(position + 1, children)
                notifyItemRangeInserted(position + 1, children.size)
                notifyItemChanged(position)
            } else {
                val nodesArr = item.subNodes ?: JSONArray()
                val count = nodesArr.length()
                for (i in 0 until count) {
                    if (position + 1 < displayList.size) {
                        displayList.removeAt(position + 1)
                    }
                }
                notifyItemRangeRemoved(position + 1, count)
                notifyItemChanged(position)
            }
        }

        private fun selectNode(name: String, link: String) {
            val prefs = requireContext().getSharedPreferences("ForgeFoxSettings", Context.MODE_PRIVATE)
            prefs.edit().putString("selected_vless", link).apply()
            
            notifyDataSetChanged()
            
            // Hotswap: if VPN is running, restart it with new node
            if (ForgeFoxVpnService.isRunning) {
                Toast.makeText(context, "Переключение на $name...", Toast.LENGTH_SHORT).show()
                val intent = android.content.Intent(requireContext(), ForgeFoxVpnService::class.java).apply {
                    action = "STOP_VPN" // we stop it, but wait, we can just send a new START intent directly!
                }
                


                try {
                    val startIntent = android.content.Intent(requireContext(), ForgeFoxVpnService::class.java).apply {
                        action = "START_VPN_SILENT"
                    }
                    requireContext().startService(startIntent)
                    
                } catch (e: Exception) {
                    Toast.makeText(context, "Ошибка переключения: ${e.message}", Toast.LENGTH_SHORT).show()
                }
            } else {
                Toast.makeText(context, "Выбран: $name", Toast.LENGTH_SHORT).show()
            }
            
            // Go back to Home tab
            val botNav = activity?.findViewById<com.google.android.material.bottomnavigation.BottomNavigationView>(R.id.bottom_nav)
            botNav?.selectedItemId = R.id.nav_home
        }

        override fun getItemCount() = displayList.size
    }

    private fun pingNode(position: Int, onComplete: (() -> Unit)? = null) {
        if (position < 0 || position >= displayList.size) {
            onComplete?.invoke()
            return
        }
        pingNodes(listOf(position), onComplete)
    }

    private fun pingAllNodes() {
        val nodesToPing = mutableListOf<Int>()
        for (i in displayList.indices) {
            val item = displayList[i]
            if (item.type != 1 && item.link.isNotEmpty() && !item.isPinging) {
                nodesToPing.add(i)
            }
        }
        if (nodesToPing.isEmpty()) return
        activity?.runOnUiThread {
            Toast.makeText(context, "Пингую ${nodesToPing.size} узлов (асинхронно)...", Toast.LENGTH_SHORT).show()
        }
        pingNodes(nodesToPing, null)
    }

    private fun pingNodes(positions: List<Int>, onComplete: (() -> Unit)?) {
        activity?.runOnUiThread {
            for (pos in positions) {
                displayList[pos].isPinging = true
                displayList[pos].pingText = "..."
                adapter.notifyItemChanged(pos)
            }
        }

        thread {
            try {
                val executor = java.util.concurrent.Executors.newFixedThreadPool(8)
                val latch = java.util.concurrent.CountDownLatch(positions.size)

                for (pos in positions) {
                    executor.execute {
                        val item = displayList[pos]
                        var time = -1L
                        var code = 0
                        var host = ""
                        var port = 22
                        try {
                            if (item.link.startsWith("ssh://")) {
                                val withoutScheme = item.link.removePrefix("ssh://").substringBefore("#")
                                val weirdRegex = Regex("^([^:@]+)@([^:@]+):([^:@]+)@([0-9]+)$")
                                val weirdMatch = weirdRegex.find(withoutScheme)
                                if (weirdMatch != null) {
                                    host = weirdMatch.groupValues[3]
                                    port = weirdMatch.groupValues[4].toIntOrNull() ?: 22
                                } else {
                                    val lastAt = withoutScheme.lastIndexOf('@')
                                    if (lastAt != -1) {
                                        val hostPortPart = withoutScheme.substring(lastAt + 1)
                                        host = hostPortPart.substringBefore(':')
                                        port = hostPortPart.substringAfter(':', "22").toIntOrNull() ?: 22
                                    } else {
                                        host = withoutScheme.substringBefore(':')
                                        port = withoutScheme.substringAfter(':', "22").toIntOrNull() ?: 22
                                    }
                                }
                                
                                val start = System.currentTimeMillis()
                                val socket = java.net.Socket()
                                socket.connect(java.net.InetSocketAddress(host, port), 8000)
                                time = System.currentTimeMillis() - start
                                socket.close()
                            } else {
                                code = 400
                            }
                        } catch (e: Exception) {
                            code = 500
                            android.util.Log.e("ForgeFoxPing", "Ping failed for $host:$port", e)
                        }

                        activity?.runOnUiThread {
                            if (time >= 0) {
                                item.pingText = "${time}ms"
                            } else {
                                item.pingText = if (code > 0) "Err $code" else "Timeout"
                            }
                            item.isPinging = false
                            adapter.notifyItemChanged(pos)
                        }
                        latch.countDown()
                    }
                }

                latch.await()
                executor.shutdown()
                activity?.runOnUiThread {
                    if (positions.size > 1) {
                        Toast.makeText(context, "Пинг всех узлов завершен!", Toast.LENGTH_SHORT).show()
                    }
                    onComplete?.invoke()
                }
            } catch (e: Exception) {
                activity?.runOnUiThread {
                    for (pos in positions) {
                        displayList[pos].isPinging = false
                        displayList[pos].pingText = "Fail"
                        adapter.notifyItemChanged(pos)
                    }
                    onComplete?.invoke()
                }
            }
        }
    }
}
