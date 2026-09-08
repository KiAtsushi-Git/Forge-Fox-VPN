$ErrorActionPreference = "Stop"

$appDir = "app/src/main/java/com/forgefox/vpn"
$resDir = "app/src/main/res"

# Menu
New-Item -ItemType Directory -Force -Path "$resDir/menu"
Set-Content -Path "$resDir/menu/bottom_nav_menu.xml" -Value @"
<?xml version="1.0" encoding="utf-8"?>
<menu xmlns:android="http://schemas.android.com/apk/res/android">
    <item android:id="@+id/nav_home" android:title="Home" android:icon="@android:drawable/ic_menu_compass" />
    <item android:id="@+id/nav_servers" android:title="Servers" android:icon="@android:drawable/ic_menu_sort_by_size" />
    <item android:id="@+id/nav_settings" android:title="Settings" android:icon="@android:drawable/ic_menu_manage" />
</menu>
"@

# Update activity_main.xml to use Fragments
Set-Content -Path "$resDir/layout/activity_main.xml" -Value @"
<?xml version="1.0" encoding="utf-8"?>
<androidx.constraintlayout.widget.ConstraintLayout 
    xmlns:android="http://schemas.android.com/apk/res/android"
    xmlns:app="http://schemas.android.com/apk/res-auto"
    android:layout_width="match_parent"
    android:layout_height="match_parent"
    android:background="#09090B">

    <androidx.fragment.app.FragmentContainerView
        android:id="@+id/nav_host_fragment"
        android:layout_width="0dp"
        android:layout_height="0dp"
        app:layout_constraintTop_toTopOf="parent"
        app:layout_constraintBottom_toTopOf="@+id/bottom_nav"
        app:layout_constraintStart_toStartOf="parent"
        app:layout_constraintEnd_toEndOf="parent" />

    <com.google.android.material.bottomnavigation.BottomNavigationView
        android:id="@+id/bottom_nav"
        android:layout_width="0dp"
        android:layout_height="wrap_content"
        android:background="#18181B"
        app:itemIconTint="#A1A1AA"
        app:itemTextColor="#A1A1AA"
        app:menu="@menu/bottom_nav_menu"
        app:layout_constraintBottom_toBottomOf="parent"
        app:layout_constraintStart_toStartOf="parent"
        app:layout_constraintEnd_toEndOf="parent" />

</androidx.constraintlayout.widget.ConstraintLayout>
"@

# Create fragment_home.xml (Moved from old activity_main)
Set-Content -Path "$resDir/layout/fragment_home.xml" -Value @"
<?xml version="1.0" encoding="utf-8"?>
<androidx.constraintlayout.widget.ConstraintLayout 
    xmlns:android="http://schemas.android.com/apk/res/android"
    xmlns:app="http://schemas.android.com/apk/res-auto"
    android:layout_width="match_parent"
    android:layout_height="match_parent"
    android:background="#09090B">

    <TextView
        android:id="@+id/lblLogo"
        android:layout_width="wrap_content"
        android:layout_height="wrap_content"
        android:text="FORGEFOX VPN"
        android:textColor="#FFFFFF"
        android:textSize="24sp"
        android:textStyle="bold"
        android:layout_marginTop="32dp"
        app:layout_constraintTop_toTopOf="parent"
        app:layout_constraintStart_toStartOf="parent"
        app:layout_constraintEnd_toEndOf="parent" />

    <Button
        android:id="@+id/btnConnectWheel"
        android:layout_width="200dp"
        android:layout_height="200dp"
        android:text="⏻"
        android:textSize="64sp"
        android:textColor="#FFFFFF"
        android:background="@drawable/power_wheel_off"
        app:layout_constraintTop_toTopOf="parent"
        app:layout_constraintBottom_toBottomOf="parent"
        app:layout_constraintStart_toStartOf="parent"
        app:layout_constraintEnd_toEndOf="parent" />

    <TextView
        android:id="@+id/lblStatus"
        android:layout_width="wrap_content"
        android:layout_height="wrap_content"
        android:text="ОТКЛЮЧЕНО"
        android:textColor="#A1A1AA"
        android:textSize="18sp"
        android:layout_marginTop="32dp"
        app:layout_constraintTop_toBottomOf="@+id/btnConnectWheel"
        app:layout_constraintStart_toStartOf="parent"
        app:layout_constraintEnd_toEndOf="parent" />

</androidx.constraintlayout.widget.ConstraintLayout>
"@

# Create fragment_settings.xml
Set-Content -Path "$resDir/layout/fragment_settings.xml" -Value @"
<?xml version="1.0" encoding="utf-8"?>
<ScrollView xmlns:android="http://schemas.android.com/apk/res/android"
    android:layout_width="match_parent"
    android:layout_height="match_parent"
    android:background="#09090B">
    <LinearLayout
        android:layout_width="match_parent"
        android:layout_height="wrap_content"
        android:orientation="vertical"
        android:padding="16dp">
        
        <TextView android:text="Settings" android:textColor="#FFFFFF" android:textSize="24sp" android:layout_width="wrap_content" android:layout_height="wrap_content" android:layout_marginBottom="16dp"/>
        
        <Switch android:id="@+id/switchAdblock" android:text="Adblock (OISD)" android:textColor="#FFFFFF" android:layout_width="match_parent" android:layout_height="wrap_content" android:layout_marginBottom="16dp"/>
        <Switch android:id="@+id/switchSplit" android:text="Split Tunneling" android:textColor="#FFFFFF" android:layout_width="match_parent" android:layout_height="wrap_content" android:layout_marginBottom="8dp"/>
        
        <TextView android:text="Bypass Domains (comma separated):" android:textColor="#A1A1AA" android:layout_width="wrap_content" android:layout_height="wrap_content"/>
        <EditText android:id="@+id/editDomains" android:textColor="#FFFFFF" android:backgroundTint="#3F3F46" android:layout_width="match_parent" android:layout_height="wrap_content" android:layout_marginBottom="16dp"/>
        
        <Switch android:id="@+id/switchDoubleHop" android:text="Double Hop (Entry Node)" android:textColor="#FFFFFF" android:layout_width="match_parent" android:layout_height="wrap_content" android:layout_marginBottom="8dp"/>
        <TextView android:text="Entry VLESS Link:" android:textColor="#A1A1AA" android:layout_width="wrap_content" android:layout_height="wrap_content"/>
        <EditText android:id="@+id/editEntryNode" android:textColor="#FFFFFF" android:backgroundTint="#3F3F46" android:layout_width="match_parent" android:layout_height="wrap_content" android:layout_marginBottom="16dp"/>

        <Button android:id="@+id/btnSave" android:text="SAVE SETTINGS" android:backgroundTint="#FF6B00" android:layout_width="match_parent" android:layout_height="wrap_content"/>
    </LinearLayout>
</ScrollView>
"@

# Create fragment_servers.xml
Set-Content -Path "$resDir/layout/fragment_servers.xml" -Value @"
<?xml version="1.0" encoding="utf-8"?>
<RelativeLayout xmlns:android="http://schemas.android.com/apk/res/android"
    android:layout_width="match_parent"
    android:layout_height="match_parent"
    android:background="#09090B">
    <TextView android:id="@+id/title" android:text="Servers" android:textColor="#FFFFFF" android:textSize="24sp" android:layout_width="wrap_content" android:layout_height="wrap_content" android:layout_margin="16dp"/>
    <androidx.recyclerview.widget.RecyclerView
        android:id="@+id/rvServers"
        android:layout_width="match_parent"
        android:layout_height="match_parent"
        android:layout_below="@id/title"
        android:padding="8dp"/>
    <com.google.android.material.floatingactionbutton.FloatingActionButton
        android:id="@+id/fabAdd"
        android:layout_width="wrap_content"
        android:layout_height="wrap_content"
        android:layout_alignParentBottom="true"
        android:layout_alignParentRight="true"
        android:layout_margin="16dp"
        android:src="@android:drawable/ic_input_add"
        android:backgroundTint="#FF6B00" />
</RelativeLayout>
"@

# Create item_server.xml
Set-Content -Path "$resDir/layout/item_server.xml" -Value @"
<?xml version="1.0" encoding="utf-8"?>
<androidx.cardview.widget.CardView xmlns:android="http://schemas.android.com/apk/res/android"
    xmlns:app="http://schemas.android.com/apk/res-auto"
    android:layout_width="match_parent"
    android:layout_height="wrap_content"
    android:layout_margin="8dp"
    app:cardBackgroundColor="#18181B"
    app:cardCornerRadius="12dp">
    <LinearLayout android:orientation="vertical" android:padding="16dp" android:layout_width="match_parent" android:layout_height="wrap_content">
        <TextView android:id="@+id/tvName" android:text="Server Name" android:textColor="#FFFFFF" android:textSize="18sp" android:textStyle="bold" android:layout_width="wrap_content" android:layout_height="wrap_content"/>
        <TextView android:id="@+id/tvPing" android:text="Ping: -- ms" android:textColor="#10B981" android:layout_width="wrap_content" android:layout_height="wrap_content" android:layout_marginTop="4dp"/>
    </LinearLayout>
</androidx.cardview.widget.CardView>
"@

Write-Host "UI XMLs generated successfully!"
