package com.forgefox.vpn

import android.os.Bundle
import android.view.LayoutInflater
import android.view.View
import android.view.ViewGroup
import android.widget.TextView
import androidx.fragment.app.Fragment
import java.util.Timer
import java.util.TimerTask

class LogsFragment : Fragment() {
    private lateinit var tvLogs: TextView
    private var timer: Timer? = null

    override fun onCreateView(
        inflater: LayoutInflater, container: ViewGroup?,
        savedInstanceState: Bundle?
    ): View? {
        val view = inflater.inflate(R.layout.fragment_logs, container, false)
        tvLogs = view.findViewById(R.id.tvLogs)
        return view
    }

    override fun onResume() {
        super.onResume()
        timer = Timer()
        timer?.schedule(object : TimerTask() {
            override fun run() {
                activity?.runOnUiThread {
                    val logsText = synchronized(ForgeFoxVpnService.logs) {
                        ForgeFoxVpnService.logs.joinToString("\n")
                    }
                    tvLogs.text = if (logsText.isEmpty()) "Лог пуст..." else logsText
                }
            }
        }, 0, 1000)
    }

    override fun onPause() {
        super.onPause()
        timer?.cancel()
        timer = null
    }
}
