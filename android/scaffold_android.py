import os

dirs = [
    "app/src/main/java/com/forgefox/vpn",
    "app/src/main/res/layout",
    "app/src/main/res/values",
    "app/src/main/res/drawable",
]

for d in dirs:
    os.makedirs(os.path.join("E:/GitLab/forgefoxvpn-android", d), exist_ok=True)

files = {
    "settings.gradle.kts": """pluginManagement {
    repositories {
        google()
        mavenCentral()
        gradlePluginPortal()
    }
}
dependencyResolutionManagement {
    repositoriesMode.set(RepositoriesMode.FAIL_ON_PROJECT_REPOS)
    repositories {
        google()
        mavenCentral()
    }
}
rootProject.name = "ForgeFoxVPN"
include(":app")
""",
    "build.gradle.kts": """plugins {
    id("com.android.application") version "8.1.0" apply false
    id("org.jetbrains.kotlin.android") version "1.9.0" apply false
}
""",
    "app/build.gradle.kts": """plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

android {
    namespace = "com.forgefox.vpn"
    compileSdk = 34

    defaultConfig {
        applicationId = "com.forgefox.vpn"
        minSdk = 26
        targetSdk = 34
        versionCode = 1
        versionName = "1.0"
    }

    buildTypes {
        release {
            isMinifyEnabled = false
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
        }
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_1_8
        targetCompatibility = JavaVersion.VERSION_1_8
    }
    kotlinOptions {
        jvmTarget = "1.8"
    }
}

dependencies {
    implementation("androidx.core:core-ktx:1.12.0")
    implementation("androidx.appcompat:appcompat:1.6.1")
    implementation("com.google.android.material:material:1.10.0")
    implementation("androidx.constraintlayout:constraintlayout:2.1.4")
}
""",
    "app/src/main/AndroidManifest.xml": """<?xml version="1.0" encoding="utf-8"?>
<manifest xmlns:android="http://schemas.android.com/apk/res/android"
    package="com.forgefox.vpn">

    <uses-permission android:name="android.permission.INTERNET" />
    <uses-permission android:name="android.permission.ACCESS_NETWORK_STATE" />
    <uses-permission android:name="android.permission.FOREGROUND_SERVICE"/>
    
    <application
        android:allowBackup="true"
        android:icon="@mipmap/ic_launcher"
        android:label="@string/app_name"
        android:roundIcon="@mipmap/ic_launcher_round"
        android:supportsRtl="true"
        android:theme="@style/Theme.ForgeFoxVPN">
        <activity
            android:name=".MainActivity"
            android:exported="true">
            <intent-filter>
                <action android:name="android.intent.action.MAIN" />
                <category android:name="android.intent.category.LAUNCHER" />
            </intent-filter>
        </activity>
        
        <service android:name=".VpnServiceHandler"
                 android:permission="android.permission.BIND_VPN_SERVICE"
                 android:exported="false">
             <intent-filter>
                 <action android:name="android.net.VpnService" />
             </intent-filter>
        </service>
    </application>
</manifest>
""",
    "app/src/main/res/values/strings.xml": """<resources>
    <string name="app_name">ForgeFoxVPN</string>
</resources>
""",
    "app/src/main/res/values/colors.xml": """<resources>
    <color name="bg_dark">#09090B</color>
    <color name="bg_panel">#18181B</color>
    <color name="accent_orange">#FF6B00</color>
    <color name="text_white">#E4E4E7</color>
    <color name="text_gray">#A1A1AA</color>
</resources>
""",
    "app/src/main/res/values/themes.xml": """<resources xmlns:tools="http://schemas.android.com/tools">
    <style name="Theme.ForgeFoxVPN" parent="Theme.MaterialComponents.DayNight.NoActionBar">
        <item name="colorPrimary">@color/accent_orange</item>
        <item name="colorPrimaryVariant">@color/accent_orange</item>
        <item name="colorOnPrimary">@color/text_white</item>
        <item name="android:windowBackground">@color/bg_dark</item>
        <item name="android:textColor">@color/text_white</item>
    </style>
</resources>
""",
    "app/src/main/res/layout/activity_main.xml": """<?xml version="1.0" encoding="utf-8"?>
<LinearLayout xmlns:android="http://schemas.android.com/apk/res/android"
    android:layout_width="match_parent"
    android:layout_height="match_parent"
    android:background="@color/bg_dark"
    android:orientation="vertical"
    android:padding="20dp"
    android:gravity="center">

    <TextView
        android:id="@+id/statusText"
        android:layout_width="wrap_content"
        android:layout_height="wrap_content"
        android:text="ОТКЛЮЧЕНО"
        android:textColor="@color/text_gray"
        android:textSize="18sp"
        android:textStyle="bold"
        android:layout_marginBottom="40dp"/>

    <Button
        android:id="@+id/connectBtn"
        android:layout_width="200dp"
        android:layout_height="60dp"
        android:text="CONNECT"
        android:backgroundTint="@color/accent_orange"
        android:textColor="@color/text_white"
        android:textSize="18sp"
        android:textStyle="bold"/>

</LinearLayout>
""",
    "app/src/main/java/com/forgefox/vpn/Core.kt": """package com.forgefox.vpn

object Core {
    init {
        System.loadLibrary("rust_core")
    }

    /**
     * Parses VLESS string into JSON.
     */
    external fun parseVless(link: String): String

    /**
     * Builds sing-box configuration based on parsed VLESS JSON and mode.
     */
    external fun buildConfig(vlessJsonStr: String, mode: String): String
}
""",
    "app/src/main/java/com/forgefox/vpn/MainActivity.kt": """package com.forgefox.vpn

import android.content.Intent
import android.net.VpnService
import android.os.Bundle
import android.widget.Button
import android.widget.TextView
import androidx.appcompat.app.AppCompatActivity

class MainActivity : AppCompatActivity() {

    private lateinit var connectBtn: Button
    private lateinit var statusText: TextView
    private var isConnected = false

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(R.layout.activity_main)

        connectBtn = findViewById(R.id.connectBtn)
        statusText = findViewById(R.id.statusText)

        connectBtn.setOnClickListener {
            if (!isConnected) {
                val intent = VpnService.prepare(this)
                if (intent != null) {
                    startActivityForResult(intent, 0)
                } else {
                    startVpn()
                }
            } else {
                stopVpn()
            }
        }
    }

    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        super.onActivityResult(requestCode, resultCode, data)
        if (requestCode == 0 && resultCode == RESULT_OK) {
            startVpn()
        }
    }

    private fun startVpn() {
        // Test rust JNI core call
        val vlessJson = Core.parseVless("vless://test@127.0.0.1:443?type=tcp")
        val config = Core.buildConfig(vlessJson, "tunnel")
        println("Generated config: $config")
        
        isConnected = true
        statusText.text = "ЗАЩИЩЕНО"
        statusText.setTextColor(android.graphics.Color.parseColor("#10B981")) // Green
        connectBtn.text = "DISCONNECT"
        connectBtn.setBackgroundColor(android.graphics.Color.parseColor("#EF4444")) // Red
        
        // Start VPN service here
    }

    private fun stopVpn() {
        isConnected = false
        statusText.text = "ОТКЛЮЧЕНО"
        statusText.setTextColor(android.graphics.Color.parseColor("#A1A1AA")) // Gray
        connectBtn.text = "CONNECT"
        connectBtn.setBackgroundColor(android.graphics.Color.parseColor("#FF6B00")) // Orange
        
        // Stop VPN service here
    }
}
""",
    "app/src/main/java/com/forgefox/vpn/VpnServiceHandler.kt": """package com.forgefox.vpn

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
"""
}

for path, content in files.items():
    full_path = os.path.join("E:/GitLab/forgefoxvpn-android", path)
    with open(full_path, "w", encoding="utf-8") as f:
        f.write(content)

print("Android project scaffolding complete.")
