package com.forgefox.vpn

import android.app.AlertDialog
import android.app.DownloadManager
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.net.Uri
import android.os.Bundle
import android.os.Environment
import android.widget.Toast
import androidx.appcompat.app.AppCompatActivity
import androidx.fragment.app.Fragment
import com.google.android.material.bottomnavigation.BottomNavigationView
import java.io.File
import kotlin.concurrent.thread
import java.net.HttpURLConnection
import java.net.URL
import androidx.core.content.FileProvider

class MainActivity : AppCompatActivity() {

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        
        // Setup Crash Handler
        val prefs = getSharedPreferences("ForgeFoxCrash", Context.MODE_PRIVATE)
        val lastCrash = prefs.getString("crash_log", null)
        if (lastCrash != null) {
            AlertDialog.Builder(this)
                .setTitle("Предыдущий краш")
                .setMessage(lastCrash)
                .setPositiveButton("ОК", null)
                .show()
            prefs.edit().remove("crash_log").apply()
        }

        val defaultHandler = Thread.getDefaultUncaughtExceptionHandler()
        Thread.setDefaultUncaughtExceptionHandler { thread, throwable ->
            val stackTrace = android.util.Log.getStackTraceString(throwable)
            prefs.edit().putString("crash_log", stackTrace).commit()
            defaultHandler?.uncaughtException(thread, throwable)
        }

        setContentView(R.layout.activity_main)

        val bottomNav = findViewById<BottomNavigationView>(R.id.bottom_nav)
        loadFragment(HomeFragment())

        bottomNav.setOnItemSelectedListener { item ->
            when (item.itemId) {
                R.id.nav_home -> loadFragment(HomeFragment())
                R.id.nav_servers -> loadFragment(ServersFragment())
                R.id.nav_settings -> loadFragment(SettingsFragment())
                R.id.nav_logs -> loadFragment(LogsFragment())
            }
            true
        }

        if (intent?.getBooleanExtra("START_VPN", false) == true) {
            val homeFragment = HomeFragment()
            homeFragment.arguments = Bundle().apply { putBoolean("AUTO_START_VPN", true) }
            loadFragment(homeFragment)
        }

        // Check for updates
        checkForUpdates()
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
        if (intent.getBooleanExtra("START_VPN", false)) {
            val navHost = supportFragmentManager.findFragmentById(R.id.nav_host_fragment)
            if (navHost is HomeFragment) {
                navHost.startVpn()
            } else {
                val homeFragment = HomeFragment()
                homeFragment.arguments = Bundle().apply { putBoolean("AUTO_START_VPN", true) }
                loadFragment(homeFragment)
                findViewById<BottomNavigationView>(R.id.bottom_nav).selectedItemId = R.id.nav_home
            }
        }
    }

    private fun loadFragment(fragment: Fragment) {
        supportFragmentManager.beginTransaction()
            .replace(R.id.nav_host_fragment, fragment)
            .commit()
    }

    private fun checkForUpdates() {
        thread {
            try {
                val url = URL("https://android.forgefoxvpn.data.forgefox.ru")
                val conn = url.openConnection() as HttpURLConnection
                conn.requestMethod = "GET"
                conn.connectTimeout = 5000
                conn.readTimeout = 5000
                conn.setRequestProperty("Range", "bytes=0-0")
                
                val code = conn.responseCode
                if (code == 200 || code == 206) {
                    val lastModified = conn.getHeaderField("Last-Modified") ?: ""
                    val eTag = conn.getHeaderField("ETag") ?: ""
                    val contentLen = conn.getHeaderField("Content-Length") ?: ""
                    
                    val currentSignature = "${lastModified}_${eTag}_${contentLen}"
                    if (currentSignature == "__") return@thread // Failed to get metadata

                    val prefs = getSharedPreferences("ForgeFoxSettings", Context.MODE_PRIVATE)
                    val savedSignature = prefs.getString("last_apk_signature", "")
                    
                    if (currentSignature != savedSignature) {
                        runOnUiThread {
                            AlertDialog.Builder(this)
                                .setTitle("Доступно обновление")
                                .setMessage("Найдена новая версия приложения. Хотите обновить?")
                                .setPositiveButton("Да") { _, _ ->
                                    prefs.edit().putString("last_apk_signature", currentSignature).apply()
                                    downloadAndInstallUpdate("https://android.forgefoxvpn.data.forgefox.ru")
                                }
                                .setNegativeButton("Нет") { _, _ ->
                                    // Save it anyway so we don't annoy them again until the NEXT update
                                    prefs.edit().putString("last_apk_signature", currentSignature).apply()
                                }
                                .show()
                        }
                    }
                }
            } catch (e: Exception) {
                e.printStackTrace()
            }
        }
    }

    private fun downloadAndInstallUpdate(apkUrl: String) {
        val file = File(getExternalFilesDir(Environment.DIRECTORY_DOWNLOADS), "ForgeFoxVPN.apk")
        if (file.exists()) file.delete()

        val request = DownloadManager.Request(Uri.parse(apkUrl))
        request.setTitle("ForgeFoxVPN Update")
        request.setDescription("Скачивание обновления...")
        request.setDestinationInExternalFilesDir(this, Environment.DIRECTORY_DOWNLOADS, "ForgeFoxVPN.apk")
        request.setNotificationVisibility(DownloadManager.Request.VISIBILITY_VISIBLE_NOTIFY_COMPLETED)

        val manager = getSystemService(Context.DOWNLOAD_SERVICE) as DownloadManager
        val downloadId = manager.enqueue(request)

        val dialogView = layoutInflater.inflate(R.layout.dialog_update, null)
        val progressBar = dialogView.findViewById<android.widget.ProgressBar>(R.id.updateProgressBar)
        val lblProgress = dialogView.findViewById<android.widget.TextView>(R.id.lblUpdateProgress)

        val dialog = AlertDialog.Builder(this, R.style.DarkAlertDialog)
            .setView(dialogView)
            .setCancelable(false)
            .show()

        val handler = android.os.Handler(android.os.Looper.getMainLooper())
        var isDownloading = true

        thread {
            while (isDownloading) {
                val q = DownloadManager.Query().setFilterById(downloadId)
                val cursor = manager.query(q)
                if (cursor != null && cursor.moveToFirst()) {
                    val bytesDownloadedIdx = cursor.getColumnIndex(DownloadManager.COLUMN_BYTES_DOWNLOADED_SO_FAR)
                    val bytesTotalIdx = cursor.getColumnIndex(DownloadManager.COLUMN_TOTAL_SIZE_BYTES)
                    
                    if (bytesDownloadedIdx >= 0 && bytesTotalIdx >= 0) {
                        val downloaded = cursor.getInt(bytesDownloadedIdx)
                        val total = cursor.getInt(bytesTotalIdx)

                        if (total > 0) {
                            val progress = (downloaded * 100L / total).toInt()
                            handler.post {
                                progressBar.progress = progress
                                lblProgress.text = "$progress%"
                            }
                        }
                    }
                    val statusIdx = cursor.getColumnIndex(DownloadManager.COLUMN_STATUS)
                    if (statusIdx >= 0) {
                        val status = cursor.getInt(statusIdx)
                        if (status == DownloadManager.STATUS_SUCCESSFUL || status == DownloadManager.STATUS_FAILED) {
                            isDownloading = false
                        }
                    }
                }
                cursor?.close()
                Thread.sleep(100)
            }
        }

        val onComplete = object : BroadcastReceiver() {
            override fun onReceive(context: Context, intent: Intent) {
                val id = intent.getLongExtra(DownloadManager.EXTRA_DOWNLOAD_ID, -1)
                if (id == downloadId) {
                    dialog.dismiss()
                    val file = File(getExternalFilesDir(Environment.DIRECTORY_DOWNLOADS), "ForgeFoxVPN.apk")
                    if (file.exists()) {
                        val installIntent = Intent(Intent.ACTION_VIEW)
                        installIntent.setDataAndType(
                            FileProvider.getUriForFile(context, "${applicationContext.packageName}.provider", file),
                            "application/vnd.android.package-archive"
                        )
                        installIntent.flags = Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_GRANT_READ_URI_PERMISSION
                        startActivity(installIntent)
                    } else {
                        Toast.makeText(context, "Файл обновления не найден", Toast.LENGTH_SHORT).show()
                    }
                }
            }
        }
        if (android.os.Build.VERSION.SDK_INT >= android.os.Build.VERSION_CODES.TIRAMISU) {
            registerReceiver(onComplete, IntentFilter(DownloadManager.ACTION_DOWNLOAD_COMPLETE), Context.RECEIVER_EXPORTED)
        } else {
            registerReceiver(onComplete, IntentFilter(DownloadManager.ACTION_DOWNLOAD_COMPLETE))
        }
    }
}
