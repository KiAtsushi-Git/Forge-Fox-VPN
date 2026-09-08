import os
import re
import subprocess
import psutil
import sys
import math
import random
import base64
import urllib.request
from urllib.parse import unquote
from PyQt6.QtWidgets import (
    QApplication, QMainWindow, QWidget, QVBoxLayout, QHBoxLayout,
    QPushButton, QTextEdit, QLineEdit, QLabel, QStackedWidget,
    QFrame, QGraphicsDropShadowEffect, QSizePolicy, QScrollArea, QComboBox, QListWidget,
    QSystemTrayIcon, QMenu, QInputDialog
)
from PyQt6.QtCore import QThread, pyqtSignal, Qt, QTimer, QPropertyAnimation, pyqtProperty, QRectF, QEasingCurve, QSettings
from PyQt6.QtGui import QColor, QFont, QCursor, QPainter, QBrush, QPen, QPainterPath, QIcon, QAction, QLinearGradient

from package.utils import resource_path
from package.core import SecureStorage, VPNManager, VPNThread, NetworkTester, ObfuscatorThread
from PyQt6.QtNetwork import QLocalServer

STYLE_SHEET = """
QFrame#MainFrame {
    background-color: #09090B;
    border-radius: 15px;
    border: 1px solid #27272A;
}
QMainWindow { background-color: #09090B; }
QWidget { font-family: 'Segoe UI', sans-serif; color: #E4E4E7; }

QListWidget#Sidebar { background-color: #18181B; border: none; border-right: 1px solid #27272A; outline: none; }
QListWidget#Sidebar::item { color: #A1A1AA; padding: 18px 25px; font-size: 15px; font-weight: 600; border-left: 3px solid transparent; }
QListWidget#Sidebar::item:hover { color: #FFFFFF; background-color: #27272A; }
QListWidget#Sidebar::item:selected { color: #FF6B00; background-color: #27272A; border-left: 3px solid #FF6B00; }

QFrame#CyberCard, QFrame#CyberPanel { background-color: #18181B; border-radius: 20px; border: 1px solid #27272A; }
QLineEdit { background-color: #09090B; border: 2px solid #27272A; border-radius: 10px; padding: 15px; color: #FFF; font-size: 14px; font-weight: 500; }
QLineEdit:focus { border: 2px solid #FF6B00; }

QComboBox#ServerDropdown { background-color: #27272A; border: 2px solid #3F3F46; border-radius: 12px; padding: 15px 20px; color: #FFF; font-size: 16px; font-weight: bold; }
QComboBox#ServerDropdown:hover { border: 2px solid #FF6B00; }
QComboBox#ServerDropdown::drop-down { border: none; }
QComboBox QAbstractItemView { background-color: #18181B; border: 1px solid #3F3F46; border-radius: 8px; selection-background-color: rgba(255, 107, 0, 0.2); selection-color: #FF6B00; outline: none; }
QComboBox QAbstractItemView::item { padding: 15px; font-size: 14px; font-weight: bold; border-radius: 6px; }

QPushButton#PingBtn { background-color: #18181B; border: 2px solid #3F3F46; border-radius: 12px; padding: 15px; color: #A1A1AA; font-weight: bold; font-size: 14px; }
QPushButton#PingBtn:hover { border-color: #FF6B00; color: #FFF; }

QPushButton#PrimaryBtn { background-color: #FF6B00; color: #FFF; border-radius: 10px; padding: 15px; font-weight: 800; font-size: 14px; }
QPushButton#PrimaryBtn:hover { background-color: #FF8533; }
QPushButton#DangerBtn { background-color: rgba(239, 68, 68, 0.1); color: #EF4444; border: 1px solid #EF4444; border-radius: 6px; padding: 8px 12px; font-weight: bold; font-size: 12px; }
QPushButton#DangerBtn:hover { background-color: #EF4444; color: #FFF; }

QPushButton#ModeBtnActive { background-color: #FF6B00; color: #FFF; border-radius: 8px; padding: 10px 20px; font-weight: 800; }
QPushButton#ModeBtnInactive { background-color: transparent; color: #71717A; border: 2px solid #27272A; border-radius: 8px; padding: 10px 20px; font-weight: bold; }
QPushButton#ModeBtnInactive:hover { color: #FFF; border: 2px solid #52525B; }
QPushButton#GhostBtn { background: transparent; color: #71717A; font-size: 20px; font-weight: bold; border-radius: 5px; }
QPushButton#GhostBtn:hover { color: #FFF; background: #27272A; }

QPushButton#TestBtn { background: #09090B; border: 1px solid #3F3F46; color: #A1A1AA; border-radius: 6px; padding: 8px 15px; font-size: 12px; font-weight: bold; }
QPushButton#TestBtn:hover { border-color: #FF6B00; color: #FFF; }

QTextEdit#Terminal { background-color: #09090B; border: 1px solid #27272A; border-radius: 12px; padding: 15px; font-family: 'Consolas', monospace; color: #A1A1AA; font-size: 13px; }
QScrollBar:vertical { background: transparent; width: 8px; margin: 0px; }
QScrollBar::handle:vertical { background: #3F3F46; border-radius: 4px; }
QScrollBar::handle:vertical:hover { background: #FF6B00; }
QScrollBar::add-line:vertical, QScrollBar::sub-line:vertical { height: 0px; }

QScrollBar:horizontal { background: transparent; height: 8px; margin: 0px; }
QScrollBar::handle:horizontal { background: #3F3F46; border-radius: 4px; }
QScrollBar::handle:horizontal:hover { background: #FF6B00; }
QScrollBar::add-line:horizontal, QScrollBar::sub-line:horizontal { width: 0px; }

QFrame#ServerListContainer {
    background-color: #18181B;
    border-bottom-left-radius: 12px;
    border-bottom-right-radius: 12px;
    border: 1px solid #27272A;
    border-top: none;
}
QFrame#ServerRow {
    background-color: transparent;
    border: none;
    border-bottom: 1px solid #27272A;
    border-radius: 0px;
}
QFrame#ServerRow:hover {
    background-color: rgba(255, 107, 0, 0.05);
}
QLabel#ServerName {
    color: #E4E4E7;
    font-size: 15px;
    font-weight: bold;
    background: transparent;
}
QLabel#ServerSub {
    color: #71717A;
    font-size: 11px;
    font-weight: 600;
    background: transparent;
}
QLabel#PingLabel {
    color: #A1A1AA;
    font-size: 13px;
    font-weight: bold;
    background: transparent;
}
QPushButton#ActionIcon {
    background: transparent;
    color: #A1A1AA;
    font-size: 16px;
    border: none;
    border-radius: 5px;
}
QPushButton#ActionIcon:hover {
    color: #FF6B00;
    background: #27272A;
}

QFrame#StatsCard {
    background-color: #18181B;
    border-radius: 12px;
    border: 1px solid #27272A;
}
QLabel#StatsTitle {
    color: #A1A1AA;
    font-size: 13px;
    font-weight: bold;
    text-transform: uppercase;
    letter-spacing: 1px;
}
QLabel#StatsValue {
    color: #FFFFFF;
    font-size: 32px;
    font-weight: 900;
}
QLabel#StatsSub {
    color: #10B981;
    font-size: 14px;
    font-weight: bold;
}
"""


class ServerSelectorPopup(QWidget):
    server_selected = pyqtSignal(int)

    def __init__(self, parent=None):
        super().__init__(parent, Qt.WindowType.Popup | Qt.WindowType.FramelessWindowHint)
        self.setAttribute(Qt.WidgetAttribute.WA_TranslucentBackground)
        self.setFixedSize(380, 450)

        layout = QVBoxLayout(self)
        layout.setContentsMargins(0, 0, 0, 0)

        self.frame = QFrame()
        self.frame.setStyleSheet("""
            QFrame {
                background-color: #121214;
                border: 2px solid #FF6B00;
                border-radius: 12px;
            }
        """)
        frame_layout = QVBoxLayout(self.frame)
        frame_layout.setContentsMargins(10, 10, 10, 10)

        self.scroll = QScrollArea()
        self.scroll.setWidgetResizable(True)
        self.scroll.setStyleSheet("""
            QScrollArea { background: transparent; border: none; }
            QScrollBar:vertical { background: transparent; width: 6px; }
            QScrollBar::handle:vertical { background: #3F3F46; border-radius: 3px; }
            QScrollBar::handle:vertical:hover { background: #FF6B00; }
        """)

        self.container = QWidget()
        self.container.setStyleSheet("background: transparent; border: none;")
        self.container_layout = QVBoxLayout(self.container)
        self.container_layout.setAlignment(Qt.AlignmentFlag.AlignTop)
        self.container_layout.setSpacing(5)

        self.scroll.setWidget(self.container)
        frame_layout.addWidget(self.scroll)
        layout.addWidget(self.frame)

    def update_list(self, servers):
        for i in reversed(range(self.container_layout.count())):
            w = self.container_layout.itemAt(i).widget()
            if w: w.setParent(None)

        if not servers:
            lbl = QLabel("Нет доступных серверов")
            lbl.setStyleSheet("color: #71717A; font-weight: bold; padding: 20px; border: none;")
            lbl.setAlignment(Qt.AlignmentFlag.AlignCenter)
            self.container_layout.addWidget(lbl)
            return

        grouped = {}
        for original_idx, s in enumerate(servers):
            g = s.get("group", "Мои узлы")
            if g not in grouped: grouped[g] = []
            grouped[g].append((original_idx, s))

        for group_name, items in grouped.items():
            accordion = CollapsibleGroup(f"{group_name} ({len(items)})", is_expanded=True)

            for original_idx, s in items:
                btn = QPushButton(s["name"])
                btn.setCursor(QCursor(Qt.CursorShape.PointingHandCursor))
                btn.setStyleSheet("""
                    QPushButton { 
                        background: #18181B; color: #FFF; padding: 12px; 
                        border-radius: 6px; text-align: left;
                        border: 1px solid #27272A;
                        font-weight: bold; font-size: 13px;
                    }
                    QPushButton:hover { background: #27272A; border: 1px solid #FF6B00; }
                """)
                btn.clicked.connect(lambda *args, i=original_idx: self._select_and_close(i))
                accordion.add_widget(btn)

            self.container_layout.addWidget(accordion)

    def _select_and_close(self, idx):
        self.server_selected.emit(idx)
        self.hide()


class HomeView(QWidget):
    request_connect = pyqtSignal()
    request_disconnect = pyqtSignal()
    request_mode = pyqtSignal(str)
    server_selected = pyqtSignal(int)
    request_quick_ping = pyqtSignal()

    def __init__(self):
        super().__init__()
        self.settings = QSettings("ForgeFox", "VPNClient")
        self.servers = []

        main_layout = QVBoxLayout(self)
        main_layout.addStretch(1)
        h_center = QHBoxLayout()
        h_center.addStretch(1)

        self.card = QFrame()
        self.card.setObjectName("CyberCard")
        self.card.setMinimumSize(500, 600)
        self.card.setMaximumSize(600, 700)
        self.card.setSizePolicy(QSizePolicy.Policy.Expanding, QSizePolicy.Policy.Expanding)
        self.card.setGraphicsEffect(create_shadow(40, 15))
        card_layout = QVBoxLayout(self.card)
        card_layout.setContentsMargins(40, 30, 40, 30)

        self.lbl_big_name = QLabel("Нет узлов")
        self.lbl_big_name.setAlignment(Qt.AlignmentFlag.AlignCenter)
        self.lbl_big_name.setStyleSheet("color: #FFFFFF; font-size: 28px; font-weight: 900; margin-bottom: 5px;")
        card_layout.addWidget(self.lbl_big_name)

        self.lbl_status = QLabel("ОЖИДАНИЕ ВЫБОРА")
        self.lbl_status.setAlignment(Qt.AlignmentFlag.AlignCenter)
        self.lbl_status.setStyleSheet("color: #71717A; font-size: 13px; font-weight: bold; letter-spacing: 2px;")
        card_layout.addWidget(self.lbl_status)

        self.combo_container = QWidget()
        combo_layout = QHBoxLayout(self.combo_container)
        combo_layout.setContentsMargins(0, 15, 0, 10)

        self.btn_server_selector = QPushButton("Выбор сервера ⌄")
        self.btn_server_selector.setCursor(QCursor(Qt.CursorShape.PointingHandCursor))
        self.btn_server_selector.setStyleSheet("""
            QPushButton {
                background-color: #27272A; border: 2px solid #3F3F46; 
                border-radius: 12px; padding: 15px 20px; 
                color: #FFF; font-size: 16px; font-weight: bold; text-align: left;
            }
            QPushButton:hover { border: 2px solid #FF6B00; }
        """)
        self.btn_server_selector.clicked.connect(self._show_server_popup)

        self.server_popup = ServerSelectorPopup(self)
        self.server_popup.server_selected.connect(self._on_server_selected)

        self.btn_quick_ping = QPushButton("⚡ Ping")
        self.btn_quick_ping.setObjectName("PingBtn")
        self.btn_quick_ping.setCursor(QCursor(Qt.CursorShape.PointingHandCursor))
        self.btn_quick_ping.clicked.connect(self.request_quick_ping.emit)

        combo_layout.addWidget(self.btn_server_selector, stretch=1)
        combo_layout.addWidget(self.btn_quick_ping)
        card_layout.addWidget(self.combo_container)

        self.pure_adblock_container = QWidget()
        pure_ab_layout = QVBoxLayout(self.pure_adblock_container)
        pure_ab_layout.setContentsMargins(0, 15, 0, 10)
        self.lbl_pure_adblock = QLabel("Блокировщик рекламы")
        self.lbl_pure_adblock.setStyleSheet("color: #FFFFFF; font-size: 20px; font-weight: 900;")
        self.lbl_pure_adblock.setAlignment(Qt.AlignmentFlag.AlignCenter)
        self.inp_pure_adblock_url = QLineEdit()
        self.inp_pure_adblock_url.setPlaceholderText("Ссылка на базу .srs")
        default_url = "https://raw.githubusercontent.com/Dreista/sing-box-rule-set-cn/rule-set/filter.txt.srs"
        self.inp_pure_adblock_url.setText(self.settings.value("adblock_url", default_url))
        self.inp_pure_adblock_url.textChanged.connect(lambda t: self.settings.setValue("adblock_url", t))
        pure_ab_layout.addWidget(self.lbl_pure_adblock)
        pure_ab_layout.addWidget(self.inp_pure_adblock_url)
        self.pure_adblock_container.hide()
        card_layout.addWidget(self.pure_adblock_container)

        style = "color: #A1A1AA; font-size: 12px;"
        self.lbl_proxy_host = CopyLabel("Host: 127.0.0.1 | Port: 1080", "127.0.0.1:1080")
        self.lbl_proxy_host.setAlignment(Qt.AlignmentFlag.AlignCenter)
        self.lbl_proxy_host.setStyleSheet(style)
        card_layout.addWidget(self.lbl_proxy_host)

        self.lbl_proxy_info = CopyLabel("User: Fox | Password: Forge | Protocol: Socks5", "Fox:Forge")
        self.lbl_proxy_info.setAlignment(Qt.AlignmentFlag.AlignCenter)
        self.lbl_proxy_info.setStyleSheet(style)
        card_layout.addWidget(self.lbl_proxy_info)

        self.settings_container = QWidget()
        settings_layout = QVBoxLayout(self.settings_container)
        settings_layout.setContentsMargins(0, 0, 0, 0)

        split_layout = QHBoxLayout()
        split_layout.setContentsMargins(0, 15, 0, 10)
        lbl_split = QLabel("Исключения (Split-Tunneling)")
        lbl_split.setStyleSheet("color: #A1A1AA; font-size: 14px; font-weight: bold;")
        is_split_on = self.settings.value("split_enabled", False, type=bool)
        self.toggle_split = AnimatedToggle(checked=is_split_on)
        self.toggle_split.toggled.connect(self._on_split_toggled)
        split_layout.addWidget(lbl_split)
        split_layout.addStretch(1)
        split_layout.addWidget(self.toggle_split)
        settings_layout.addLayout(split_layout)

        adblock_layout = QHBoxLayout()
        adblock_layout.setContentsMargins(0, 0, 0, 10)
        lbl_adblock = QLabel("Блокировка рекламы (AdBlock)")
        lbl_adblock.setStyleSheet("color: #A1A1AA; font-size: 14px; font-weight: bold;")
        is_adblock_on = self.settings.value("adblock_enabled", False, type=bool)
        self.toggle_adblock = AnimatedToggle(checked=is_adblock_on)
        self.toggle_adblock.toggled.connect(self._on_adblock_toggled)
        adblock_layout.addWidget(lbl_adblock)
        adblock_layout.addStretch(1)
        adblock_layout.addWidget(self.toggle_adblock)
        settings_layout.addLayout(adblock_layout)

        obfuscator_layout = QHBoxLayout()
        obfuscator_layout.setContentsMargins(0, 0, 0, 10)
        lbl_obfuscator = QLabel("Маскировка трафика (Белый шум)")
        lbl_obfuscator.setStyleSheet("color: #A1A1AA; font-size: 14px; font-weight: bold;")
        is_obf_on = self.settings.value("obfuscator_enabled", False, type=bool)
        self.toggle_obfuscator = AnimatedToggle(checked=is_obf_on)
        self.toggle_obfuscator.toggled.connect(self._on_obfuscator_toggled)
        obfuscator_layout.addWidget(lbl_obfuscator)
        obfuscator_layout.addStretch(1)
        obfuscator_layout.addWidget(self.toggle_obfuscator)
        settings_layout.addLayout(obfuscator_layout)

        card_layout.addWidget(self.settings_container)

        self.wave = WaveVisualizer()
        card_layout.addWidget(self.wave)
        card_layout.addStretch(1)

        wheel_layout = QHBoxLayout()
        self.wheel = PowerWheel()
        self.wheel.clicked.connect(self._on_wheel_click)
        wheel_layout.addWidget(self.wheel, alignment=Qt.AlignmentFlag.AlignCenter)
        card_layout.addLayout(wheel_layout)
        card_layout.addStretch(1)

        mode_layout = QHBoxLayout()
        mode_layout.setSpacing(10)

        self.btn_proxy = QPushButton("PROXY")
        self.btn_proxy.setObjectName("ModeBtnActive")
        self.btn_proxy.setCursor(QCursor(Qt.CursorShape.PointingHandCursor))

        self.btn_tun = QPushButton("TUNNEL")
        self.btn_tun.setObjectName("ModeBtnInactive")
        self.btn_tun.setCursor(QCursor(Qt.CursorShape.PointingHandCursor))

        self.btn_adblock = QPushButton("ADBLOCK")
        self.btn_adblock.setObjectName("ModeBtnInactive")
        self.btn_adblock.setCursor(QCursor(Qt.CursorShape.PointingHandCursor))

        self.btn_proxy.clicked.connect(lambda: self.set_mode("proxy"))
        self.btn_tun.clicked.connect(lambda: self.set_mode("tunnel"))
        self.btn_adblock.clicked.connect(lambda: self.set_mode("adblock"))

        mode_layout.addWidget(self.btn_proxy)
        mode_layout.addWidget(self.btn_tun)
        mode_layout.addWidget(self.btn_adblock)
        card_layout.addLayout(mode_layout)

        h_center.addWidget(self.card)
        h_center.addStretch(1)
        main_layout.addLayout(h_center)
        main_layout.addStretch(1)

    def _show_server_popup(self):
        pos = self.btn_server_selector.mapToGlobal(self.btn_server_selector.rect().bottomLeft())
        self.server_popup.setFixedWidth(self.btn_server_selector.width())
        self.server_popup.move(pos.x(), pos.y() + 5)
        self.server_popup.show()

    def _on_server_selected(self, idx):
        self.server_selected.emit(idx)
        self.btn_quick_ping.setText("⚡ Ping")
        self.btn_quick_ping.setStyleSheet("")

        name = self.servers[idx]["name"]
        self.btn_server_selector.setText(f"{name}  ⌄")
        self.lbl_big_name.setText(name)

    def update_combo_list(self, servers):
        self.servers = servers
        self.server_popup.update_list(servers)

        if not servers:
            self.btn_server_selector.setText("Нет серверов  ⌄")
            self.btn_server_selector.setEnabled(False)
            self.btn_quick_ping.setEnabled(False)
            self.lbl_big_name.setText("Нет узлов")
        else:
            self.btn_server_selector.setEnabled(True)
            self.btn_quick_ping.setEnabled(True)

            first_idx = 0
            self.btn_server_selector.setText(f"{servers[first_idx]['name']}  ⌄")
            self.lbl_big_name.setText(servers[first_idx]['name'])

    def _on_obfuscator_toggled(self, state):
        self.settings.setValue("obfuscator_enabled", state)
        self.settings.sync()

    def _on_wheel_click(self):
        if self.wheel.state == "off" or self.wheel.state == "loading":
            self.wheel.set_state("loading")
            self.lbl_status.setText("УСТАНОВКА СОЕДИНЕНИЯ...")
            self.lbl_status.setStyleSheet("color: #FF6B00; font-weight: bold; letter-spacing: 2px;")

            self.btn_proxy.setEnabled(False)
            self.btn_tun.setEnabled(False)
            self.btn_adblock.setEnabled(False)
            self.btn_server_selector.setEnabled(False)

            self.request_connect.emit()

        elif self.wheel.state == "on":
            self.wheel.set_state("loading")
            self.lbl_status.setText("ОТКЛЮЧЕНИЕ...")
            self.lbl_status.setStyleSheet("color: #FF6B00; font-weight: bold; letter-spacing: 2px;")

            self.btn_proxy.setEnabled(False)
            self.btn_tun.setEnabled(False)
            self.btn_adblock.setEnabled(False)
            self.btn_server_selector.setEnabled(False)

            self.request_disconnect.emit()

    def set_mode(self, mode):
        self.request_mode.emit(mode)
        self.btn_proxy.setObjectName("ModeBtnActive" if mode == "proxy" else "ModeBtnInactive")
        self.btn_tun.setObjectName("ModeBtnActive" if mode == "tunnel" else "ModeBtnInactive")
        self.btn_adblock.setObjectName("ModeBtnActive" if mode == "adblock" else "ModeBtnInactive")

        self.btn_proxy.style().unpolish(self.btn_proxy);
        self.btn_proxy.style().polish(self.btn_proxy)
        self.btn_tun.style().unpolish(self.btn_tun);
        self.btn_tun.style().polish(self.btn_tun)
        self.btn_adblock.style().unpolish(self.btn_adblock);
        self.btn_adblock.style().polish(self.btn_adblock)

        self.lbl_proxy_host.setVisible(mode == "proxy")
        self.lbl_proxy_info.setVisible(mode == "proxy")

        if mode == "adblock":
            self.lbl_big_name.hide()
            self.combo_container.hide()
            self.settings_container.hide()
            self.pure_adblock_container.show()
        else:
            self.lbl_big_name.show()
            self.combo_container.show()
            self.settings_container.show()
            self.pure_adblock_container.hide()

    def set_connected_state(self, is_connected):
        if is_connected:
            self.wheel.set_state("on")
            self.lbl_status.setText(
                "ЗАЩИЩЕНО" if self.btn_adblock.objectName() != "ModeBtnActive" else "РЕКЛАМА ЗАБЛОКИРОВАНА")
            self.lbl_status.setStyleSheet("color: #10B981; font-weight: bold; letter-spacing: 2px;")
            self.btn_proxy.setEnabled(False)
            self.btn_tun.setEnabled(False)
            self.btn_adblock.setEnabled(False)
            self.btn_server_selector.setEnabled(True)
            self.toggle_split.setEnabled(False)
            self.toggle_adblock.setEnabled(False)
            self.toggle_obfuscator.setEnabled(False)
            self.wave.set_active(True)
        else:
            self.wheel.set_state("off")
            self.lbl_status.setText("ОТКЛЮЧЕНО")
            self.lbl_status.setStyleSheet("color: #71717A; font-weight: bold; letter-spacing: 2px;")
            self.btn_proxy.setEnabled(True)
            self.btn_tun.setEnabled(True)
            self.btn_adblock.setEnabled(True)
            self.toggle_split.setEnabled(True)
            self.toggle_adblock.setEnabled(True)
            self.btn_server_selector.setEnabled(True)
            self.toggle_obfuscator.setEnabled(True)
            self.wave.set_active(False)

    def set_error_state(self):
        self.wheel.set_state("off")
        self.lbl_status.setText("ОШИБКА ПОДКЛЮЧЕНИЯ")
        self.lbl_status.setStyleSheet("color: #EF4444; font-weight: bold; letter-spacing: 2px;")
        self.wave.set_active(False)

        self.btn_server_selector.setEnabled(True)
        self.btn_proxy.setEnabled(True)
        self.btn_tun.setEnabled(True)
        self.btn_adblock.setEnabled(True)

    def show_quick_ping(self, ms):
        if ms == -1:
            self.btn_quick_ping.setText("Err ❌")
            self.btn_quick_ping.setStyleSheet("color: #EF4444; border-color: #EF4444;")
        else:
            self.btn_quick_ping.setText(f"{ms} ms")
            color = "#10B981" if ms < 400 else "#F59E0B" if ms < 650 else "#EF4444"
            self.btn_quick_ping.setStyleSheet(f"color: {color}; border-color: {color};")

    def _on_split_toggled(self, state):
        self.settings.setValue("split_enabled", state)
        self.settings.sync()

    def _on_adblock_toggled(self, state):
        self.settings.setValue("adblock_enabled", state)
        self.settings.sync()


class CollapsibleGroup(QWidget):
    def __init__(self, title, is_expanded=True):
        super().__init__()
        layout = QVBoxLayout(self)
        layout.setContentsMargins(0, 0, 0, 10)
        layout.setSpacing(5)

        self.btn_toggle = QPushButton(f"{title} {'⌃' if is_expanded else '⌄'}")
        self.btn_toggle.setCursor(QCursor(Qt.CursorShape.PointingHandCursor))
        self.btn_toggle.setStyleSheet("""
            QPushButton {
                background-color: #18181B; 
                color: #A1A1AA; 
                border: 1px solid #27272A; 
                border-radius: 8px; 
                padding: 10px 15px; 
                font-size: 14px; 
                font-weight: bold; 
                text-align: left;
            }
            QPushButton:hover { background-color: #27272A; color: #FFF; border: 1px solid #3F3F46; }
        """)
        self.btn_toggle.clicked.connect(self.toggle_group)
        layout.addWidget(self.btn_toggle)

        self.content_area = QFrame()
        self.content_area.setStyleSheet("background: transparent; border: none;")
        self.content_layout = QVBoxLayout(self.content_area)
        self.content_layout.setContentsMargins(10, 5, 0, 5)
        self.content_layout.setSpacing(10)
        layout.addWidget(self.content_area)

        self.is_expanded = is_expanded
        self.content_area.setVisible(self.is_expanded)

    def add_widget(self, widget):
        self.content_layout.addWidget(widget)

    def toggle_group(self):
        self.is_expanded = not self.is_expanded
        self.content_area.setVisible(self.is_expanded)
        current_text = self.btn_toggle.text()
        if self.is_expanded:
            self.btn_toggle.setText(current_text.replace("⌄", "⌃"))
        else:
            self.btn_toggle.setText(current_text.replace("⌃", "⌄"))


class UltimateServerCard(QFrame):
    request_test = pyqtSignal(str, dict)
    request_delete = pyqtSignal(str)
    request_connect = pyqtSignal(int)
    request_favorite = pyqtSignal(str, bool)
    request_rename = pyqtSignal(str, str)

    def __init__(self, server_data, index):
        super().__init__()
        self.server_data = server_data
        self.index = index
        self.is_fav = server_data.get("fav", False)
        self.last_ping = 9999

        self.setObjectName("CyberPanel")
        self.setStyleSheet("""
            QFrame#CyberPanel { background-color: #121214; border: 1px solid #27272A; border-radius: 12px; }
            QFrame#CyberPanel:hover { border: 1px solid #FF6B00; background-color: #18181B; }
        """)

        self.setFixedHeight(85)
        self.setMaximumWidth(800)

        main_layout = QHBoxLayout(self)
        main_layout.setContentsMargins(15, 10, 15, 10)
        main_layout.setSpacing(12)

        self.btn_fav = QPushButton("⭐" if self.is_fav else "☆")
        self.btn_fav.setCursor(QCursor(Qt.CursorShape.PointingHandCursor))
        self.btn_fav.setStyleSheet("background: transparent; color: #F59E0B; font-size: 20px; border: none;")
        self.btn_fav.setFixedWidth(30)
        self.btn_fav.clicked.connect(self._toggle_fav)
        main_layout.addWidget(self.btn_fav)

        self.lbl_icon = QLabel("🌐")
        self.lbl_icon.setStyleSheet("font-size: 28px; background: transparent;")
        main_layout.addWidget(self.lbl_icon)

        info_layout = QVBoxLayout()
        info_layout.setSpacing(4)
        info_layout.setAlignment(Qt.AlignmentFlag.AlignVCenter)

        name_layout = QHBoxLayout()
        self.lbl_name = QLabel(server_data["name"])
        self.lbl_name.setStyleSheet("color: #FFF; font-size: 15px; font-weight: 900; background: transparent;")

        self.btn_rename = QPushButton("✏️")
        self.btn_rename.setFixedSize(20, 20)
        self.btn_rename.setCursor(QCursor(Qt.CursorShape.PointingHandCursor))
        self.btn_rename.setStyleSheet(
            "QPushButton { background: #27272A; border-radius: 4px; color: #A1A1AA; font-size: 10px; } QPushButton:hover { background: #3F3F46; }")
        self.btn_rename.clicked.connect(self._rename_clicked)

        self.btn_copy = QPushButton("📋")
        self.btn_copy.setFixedSize(20, 20)
        self.btn_copy.setCursor(QCursor(Qt.CursorShape.PointingHandCursor))
        self.btn_copy.setStyleSheet(
            "QPushButton { background: #27272A; border-radius: 4px; color: #A1A1AA; font-size: 10px; } QPushButton:hover { background: #3F3F46; }")
        self.btn_copy.clicked.connect(lambda: QApplication.clipboard().setText(self.server_data["link"]))

        name_layout.addWidget(self.lbl_name)
        name_layout.addWidget(self.btn_rename)
        name_layout.addWidget(self.btn_copy)
        name_layout.addStretch()
        info_layout.addLayout(name_layout)

        self.setMaximumWidth(1000)

        badges_layout = QHBoxLayout()
        badges_layout.setSpacing(6)

        try:
            v = VPNManager.parse_vless(server_data["link"])

            addr = f"{v['server']}:{v['port']}"
            net = v.get("type", "TCP").upper()
            sec = "REALITY" if v.get("pbk") else ("TLS" if v.get("sni") else "NONE")
            fp = v.get("fp", "chrome").upper()
            sni = v.get("sni", "DIRECT")

            lbl_addr = self._create_badge(addr, "transparent", "#A1A1AA")
            lbl_addr.setStyleSheet("color: #A1A1AA; font-size: 11px; font-weight: 600; padding-right: 5px;")

            lbl_net = self._create_badge(net, "rgba(16, 185, 129, 0.15)", "#6EE7B7")

            lbl_sec = self._create_badge(sec, "rgba(245, 158, 11, 0.15)", "#FCD34D")

            badges_layout.addWidget(lbl_addr)
            badges_layout.addWidget(lbl_net)

            badges_layout.addWidget(lbl_sec)

            lbl_fp = self._create_badge(f"FP: {fp}", "rgba(59, 130, 246, 0.15)", "#93C5FD")
            badges_layout.addWidget(lbl_fp)

            lbl_sni = self._create_badge(f"SNI: {sni}", "rgba(139, 92, 246, 0.15)", "#C4B5FD")
            badges_layout.addWidget(lbl_sni)

        except Exception:
            lbl_err = self._create_badge("INVALID CONFIG", "rgba(239, 68, 68, 0.15)", "#FCA5A5")
            badges_layout.addWidget(lbl_err)

        badges_layout.addStretch()
        info_layout.addLayout(badges_layout)
        main_layout.addLayout(info_layout, stretch=1)

        self.lbl_ping = QLabel("--- ms")
        self.lbl_ping.setFixedWidth(70)
        self.lbl_ping.setAlignment(Qt.AlignmentFlag.AlignRight | Qt.AlignmentFlag.AlignVCenter)
        self.lbl_ping.setStyleSheet("color: #71717A; font-size: 15px; font-weight: 900; background: transparent;")
        main_layout.addWidget(self.lbl_ping)

        self.btn_connect = QPushButton("CONNECT")
        self.btn_connect.setCursor(QCursor(Qt.CursorShape.PointingHandCursor))
        self.btn_connect.setFixedHeight(32)
        self.btn_connect.setStyleSheet(
            "QPushButton { background: rgba(255, 107, 0, 0.1); color: #FF6B00; border: 1px solid #FF6B00; border-radius: 6px; padding: 0 14px; font-weight: 900; font-size: 11px; } QPushButton:hover { background: #FF6B00; color: #FFF; }")
        self.btn_connect.clicked.connect(lambda: self.request_connect.emit(self.index))

        self.btn_ping = QPushButton("⚡")
        self.btn_ping.setFixedSize(32, 32)
        self.btn_ping.setCursor(QCursor(Qt.CursorShape.PointingHandCursor))
        self.btn_ping.setStyleSheet(
            "QPushButton { background: #27272A; color: #FFF; border-radius: 6px; } QPushButton:hover { background: #3F3F46; }")
        self.btn_ping.clicked.connect(lambda: self._run_test("get"))

        self.btn_del = QPushButton("🗑")
        self.btn_del.setFixedSize(32, 32)
        self.btn_del.setCursor(QCursor(Qt.CursorShape.PointingHandCursor))
        self.btn_del.setStyleSheet(
            "QPushButton { background: #27272A; color: #EF4444; border-radius: 6px; } QPushButton:hover { background: #EF4444; color: #FFF; }")
        self.btn_del.clicked.connect(lambda: self.request_delete.emit(self.server_data["name"]))

        main_layout.addWidget(self.btn_connect)
        main_layout.addWidget(self.btn_ping)
        main_layout.addWidget(self.btn_del)

    def _create_badge(self, text, bg, color):
        lbl = QLabel(text)
        if len(text) > 25:
            lbl.setText(text[:22] + "...")
            lbl.setToolTip(text)

        lbl.setStyleSheet(
            f"background: {bg}; color: {color}; padding: 2px 6px; border-radius: 4px; font-size: 9px; font-weight: bold;")
        return lbl

    def _rename_clicked(self):
        new_name, ok = QInputDialog.getText(self, "Переименовать узел", "Новое имя:", QLineEdit.EchoMode.Normal,
                                            self.server_data["name"])
        if ok and new_name.strip() and new_name.strip() != self.server_data["name"]:
            self.request_rename.emit(self.server_data["name"], new_name.strip())

    def _toggle_fav(self):
        self.is_fav = not self.is_fav
        self.btn_fav.setText("⭐" if self.is_fav else "☆")
        self.request_favorite.emit(self.server_data["name"], self.is_fav)

    def _run_test(self, test_type):
        self.lbl_ping.setText("Ping...")
        self.lbl_ping.setStyleSheet("color: #F59E0B; font-size: 13px; font-weight: bold; background: transparent;")
        self.request_test.emit(test_type, self.server_data)

    def update_test_result(self, test_type, ms):
        if ms == -1:
            self.last_ping = 9999
            self.lbl_ping.setText("FAIL")
            self.lbl_ping.setStyleSheet("color: #EF4444; font-size: 15px; font-weight: 900; background: transparent;")
        else:
            self.last_ping = ms
            self.lbl_ping.setText(f"{ms} ms")
            color = "#10B981" if ms < 250 else "#F59E0B" if ms < 500 else "#EF4444"
            self.lbl_ping.setStyleSheet(f"color: {color}; font-size: 15px; font-weight: 900; background: transparent;")

class SubscriptionWorker(QThread):
    """Фоновый поток для скачивания и парсинга подписок без фризов UI"""
    success = pyqtSignal(str)
    error = pyqtSignal(str)

    def __init__(self, url):
        super().__init__()
        self.url = url

    def run(self):
        try:
            req = urllib.request.Request(
                self.url,
                headers={'User-Agent': 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) ForgeFox'}
            )
            with urllib.request.urlopen(req, timeout=10) as response:
                content = response.read().decode('utf-8').strip()

            try:
                padded = content + '=' * (-len(content) % 4)
                decoded = base64.b64decode(padded).decode('utf-8')
                if "vless://" in decoded:
                    content = decoded
            except Exception:
                pass

            self.success.emit(content)

        except Exception as e:
            self.error.emit(f"Ошибка загрузки: {str(e)}")


class SubscriptionGroupCard(QFrame):
    request_update = pyqtSignal(str, str)
    request_delete_group = pyqtSignal(str)
    request_rename_group = pyqtSignal(str, str)

    def __init__(self, group_name, sub_url, node_count, is_expanded=False):
        super().__init__()
        self.group_name = group_name
        self.sub_url = sub_url
        self.setObjectName("CyberPanel")

        main_layout = QVBoxLayout(self)
        main_layout.setContentsMargins(15, 15, 15, 15)
        main_layout.setSpacing(10)

        header_layout = QHBoxLayout()

        icon_lbl = QLabel("📦" if sub_url else "🏠")
        icon_lbl.setStyleSheet("font-size: 20px; background: transparent;")
        header_layout.addWidget(icon_lbl)

        text_layout = QVBoxLayout()
        text_layout.setSpacing(2)

        top_line = QHBoxLayout()
        lbl_title = QLabel(group_name)
        lbl_title.setStyleSheet("color: #FFF; font-size: 15px; font-weight: 900; background: transparent;")
        top_line.addWidget(lbl_title)

        self.btn_rename = QPushButton("✏️")
        self.btn_rename.setFixedSize(20, 20)
        self.btn_rename.setCursor(QCursor(Qt.CursorShape.PointingHandCursor))
        self.btn_rename.setStyleSheet(
            "QPushButton { background: #27272A; border-radius: 4px; color: #A1A1AA; font-size: 10px; } QPushButton:hover { background: #3F3F46; }")
        self.btn_rename.clicked.connect(self._rename_clicked)
        top_line.addWidget(self.btn_rename)
        top_line.addStretch()

        text_layout.addLayout(top_line)

        if sub_url:
            lbl_url = QLabel(sub_url)
            lbl_url.setStyleSheet("color: #71717A; font-size: 10px; font-weight: bold; background: transparent;")
            text_layout.addWidget(lbl_url)

        header_layout.addLayout(text_layout)
        header_layout.addStretch()

        lbl_count = QLabel(f"{node_count} узлов")
        lbl_count.setStyleSheet(
            "background: rgba(16, 185, 129, 0.15); color: #10B981; padding: 4px 8px; border-radius: 6px; font-size: 11px; font-weight: bold;")
        header_layout.addWidget(lbl_count)

        if sub_url:
            self.btn_update = QPushButton("🔄")
            self.btn_update.setFixedSize(30, 30)
            self.btn_update.setCursor(QCursor(Qt.CursorShape.PointingHandCursor))
            self.btn_update.setStyleSheet(
                "QPushButton { background: #27272A; color: #FFF; border-radius: 6px; font-size: 14px;} QPushButton:hover { background: #FF6B00; }")
            self.btn_update.clicked.connect(lambda: self.request_update.emit(self.sub_url, self.group_name))
            header_layout.addWidget(self.btn_update)

        self.btn_del = QPushButton("🗑")
        self.btn_del.setFixedSize(30, 30)
        self.btn_del.setCursor(QCursor(Qt.CursorShape.PointingHandCursor))
        self.btn_del.setStyleSheet(
            "QPushButton { background: #27272A; color: #EF4444; border-radius: 6px; font-size: 14px;} QPushButton:hover { background: #EF4444; color: #FFF; }")
        self.btn_del.clicked.connect(lambda: self.request_delete_group.emit(self.group_name))
        header_layout.addWidget(self.btn_del)

        self.btn_toggle = QPushButton("⌃" if is_expanded else "⌄")
        self.btn_toggle.setFixedSize(30, 30)
        self.btn_toggle.setCursor(QCursor(Qt.CursorShape.PointingHandCursor))
        self.btn_toggle.setStyleSheet(
            "QPushButton { background: transparent; color: #A1A1AA; font-size: 16px; font-weight: bold; border: none; } QPushButton:hover { color: #FFF; }")
        self.btn_toggle.clicked.connect(self.toggle_group)
        header_layout.addWidget(self.btn_toggle)

        main_layout.addLayout(header_layout)

        self.content_area = QWidget()
        self.content_area.setStyleSheet("background: transparent;")
        self.content_layout = QVBoxLayout(self.content_area)
        self.content_layout.setContentsMargins(0, 10, 0, 0)
        self.content_layout.setSpacing(8)

        main_layout.addWidget(self.content_area)

        self.is_expanded = is_expanded
        self.content_area.setVisible(self.is_expanded)

    def add_widget(self, widget):
        self.content_layout.addWidget(widget)

    def toggle_group(self):
        self.is_expanded = not self.is_expanded
        self.content_area.setVisible(self.is_expanded)
        self.btn_toggle.setText("⌃" if self.is_expanded else "⌄")

    def _rename_clicked(self):
        new_name, ok = QInputDialog.getText(self, "Переименовать подписку", "Новое имя:", QLineEdit.EchoMode.Normal,
                                            self.group_name)
        if ok and new_name.strip() and new_name.strip() != self.group_name:
            self.request_rename_group.emit(self.group_name, new_name.strip())



class ServersView(QWidget):
    servers_updated = pyqtSignal(list)
    request_run_test = pyqtSignal(str, dict)
    fast_connect_requested = pyqtSignal(int)

    def __init__(self, storage):
        super().__init__()
        self.storage = storage
        self.servers = self.storage.load()
        self.card_widgets = {}

        self.updating_group_name = None
        self.updating_url = None

        layout = QVBoxLayout(self)
        layout.setContentsMargins(30, 20, 30, 30)
        layout.setSpacing(15)

        title = QLabel("Управление подписками и узлами")
        title.setStyleSheet("font-size: 28px; font-weight: 900; color: #FFF; letter-spacing: -1px;")
        layout.addWidget(title)

        hub_frame = QFrame()
        hub_frame.setObjectName("CyberCard")
        hub_layout = QVBoxLayout(hub_frame)
        hub_layout.setContentsMargins(20, 20, 20, 20)
        hub_layout.setSpacing(15)

        input_layout = QHBoxLayout()
        self.inp_link = QLineEdit()
        self.inp_link.setPlaceholderText("Вставьте vless:// (один узел) или http:// (ссылка на подписку)...")

        self.btn_add = QPushButton("Добавить")
        self.btn_add.setObjectName("PrimaryBtn")
        self.btn_add.setCursor(QCursor(Qt.CursorShape.PointingHandCursor))

        self.btn_import_clip = QPushButton("📋 Из буфера")
        self.btn_import_clip.setObjectName("TestBtn")
        self.btn_import_clip.setCursor(QCursor(Qt.CursorShape.PointingHandCursor))

        input_layout.addWidget(self.inp_link, stretch=1)
        input_layout.addWidget(self.btn_add)
        input_layout.addWidget(self.btn_import_clip)
        hub_layout.addLayout(input_layout)

        tools_layout = QHBoxLayout()

        self.inp_search = QLineEdit()
        self.inp_search.setPlaceholderText("🔍 Поиск сервера...")
        self.inp_search.setFixedWidth(200)
        self.inp_search.textChanged.connect(self._filter_list)

        self.combo_sort = QComboBox()
        self.combo_sort.setObjectName("ServerDropdown")
        self.combo_sort.setFixedWidth(220)
        self.combo_sort.addItems(["⭐ По умолчанию", "📶 По Пингу", "🔤 По Алфавиту"])
        self.combo_sort.currentIndexChanged.connect(self._sort_list)

        self.btn_test_all = QPushButton("⚡ Пинг всех")
        self.btn_test_all.setObjectName("TestBtn")
        self.btn_test_all.setCursor(QCursor(Qt.CursorShape.PointingHandCursor))

        self.btn_find_best = QPushButton("🏆 Найти лучший")
        self.btn_find_best.setStyleSheet(
            "QPushButton { background: rgba(16, 185, 129, 0.2); color: #10B981; border: 1px solid #10B981; border-radius: 6px; padding: 8px 15px; font-weight: bold; } QPushButton:hover { background: #10B981; color: #FFF; }")
        self.btn_find_best.setCursor(QCursor(Qt.CursorShape.PointingHandCursor))

        tools_layout.addWidget(self.inp_search)
        tools_layout.addWidget(self.combo_sort)
        tools_layout.addStretch()
        tools_layout.addWidget(self.btn_test_all)
        tools_layout.addWidget(self.btn_find_best)

        hub_layout.addLayout(tools_layout)
        layout.addWidget(hub_frame)

        self.list_container = QWidget()
        self.list_container.setStyleSheet("background: transparent;")

        self.cards_layout = QVBoxLayout(self.list_container)
        self.cards_layout.setContentsMargins(0, 10, 0, 10)
        self.cards_layout.setSpacing(15)
        self.cards_layout.setAlignment(Qt.AlignmentFlag.AlignTop | Qt.AlignmentFlag.AlignHCenter)

        scroll = QScrollArea()
        scroll.setWidgetResizable(True)
        scroll.setFrameShape(QFrame.Shape.NoFrame)
        scroll.setStyleSheet("QScrollArea { background: transparent; border: none; }")
        scroll.setWidget(self.list_container)
        layout.addWidget(scroll)

        self.btn_add.clicked.connect(self._process_input)
        self.btn_import_clip.clicked.connect(self._import_clipboard)
        self.btn_test_all.clicked.connect(self._test_all)
        self.btn_find_best.clicked.connect(self._find_best)

        self._refresh_list()

    def _process_input(self):
        text = self.inp_link.text().strip()
        if not text: return
        self._handle_raw_input(text)
        self.inp_link.clear()

    def _import_clipboard(self):
        text = QApplication.clipboard().text().strip()
        self._handle_raw_input(text)

    def _handle_raw_input(self, text):
        from urllib.parse import urlparse
        if text.startswith("http://") or text.startswith("https://"):
            try:
                parsed = urlparse(text)
                default_name = parsed.netloc if parsed.netloc else "Подписка"
                if default_name.startswith("www."):
                    default_name = default_name[4:]
            except:
                default_name = "Подписка"

            self.updating_group_name = default_name
            self.updating_url = text
            self._fetch_subscription(text)
        else:
            self._parse_and_add_links(text, group_name="Мои узлы", sub_url=None)

    def _fetch_subscription(self, url):
        self.btn_add.setText("⏳ Загрузка...")
        self.btn_add.setEnabled(False)
        self.inp_link.setPlaceholderText(f"Скачивание {url[:30]}...")

        self.sub_worker = SubscriptionWorker(url)
        self.sub_worker.success.connect(self._on_subscription_success)
        self.sub_worker.error.connect(self._on_subscription_error)
        self.sub_worker.start()

    def _on_subscription_success(self, content):
        self.btn_add.setText("Добавить")
        self.btn_add.setEnabled(True)
        self.inp_link.setPlaceholderText("Вставьте vless:// или ссылку на подписку (http/https)...")

        self._parse_and_add_links(content, group_name=self.updating_group_name, sub_url=self.updating_url)

        self.updating_group_name = None
        self.updating_url = None

    def _on_subscription_error(self, error_msg):
        self.btn_add.setText("Добавить")
        self.btn_add.setEnabled(True)
        self.inp_link.setPlaceholderText("❌ Ошибка загрузки подписки!")
        self.updating_group_name = None
        self.updating_url = None
        QTimer.singleShot(3000, lambda: self.inp_link.setPlaceholderText(
            "Вставьте vless:// или ссылку на подписку (http/https)..."))

    def _parse_and_add_links(self, text, group_name="Мои узлы", sub_url=None):
        parts = re.split(r'(?=vless://)', text)
        links = [p.strip() for p in parts if p.strip().startswith('vless://')]

        if sub_url:
            self.servers = [s for s in self.servers if s.get("group") != group_name]

        count_added = 0
        for link in links:
            name = f"Node-{random.randint(1000, 9999)}"

            if '#' in link:
                name = unquote(link.split('#')[-1]).strip()

            self.servers.append({
                "name": name,
                "link": link,
                "fav": False,
                "group": group_name,
                "sub_url": sub_url
            })
            count_added += 1

        self.storage.save(self.servers)
        self._refresh_list()

        if count_added > 0:
            self.inp_link.setPlaceholderText(f"✅ Добавлено/Обновлено узлов: {count_added} в '{group_name}'")
        else:
            self.inp_link.setPlaceholderText("✅ Новых узлов не найдено.")

        QTimer.singleShot(4000, lambda: self.inp_link.setPlaceholderText(
            "Вставьте vless:// или ссылку на подписку (http/https)..."))

    def _trigger_update_subscription(self, url, group_name):
        """Вызывается при нажатии кнопки 'Обновить' на карточке подписки"""
        self.updating_url = url
        self.updating_group_name = group_name
        self._fetch_subscription(url)

    def _trigger_delete_group(self, group_name):
        """Удаляет всю подписку (группу) целиком"""
        self.servers = [s for s in self.servers if s.get("group", "Мои узлы") != group_name]
        self.storage.save(self.servers)
        self._refresh_list()

    def _rename_group(self, old_name, new_name):
        """Переименовывает подписку у всех входящих в неё серверов"""
        if not new_name.strip() or old_name == new_name:
            return

        for s in self.servers:
            if s.get("group", "Мои узлы") == old_name:
                s["group"] = new_name.strip()

        self.storage.save(self.servers)
        self._refresh_list()

    def _refresh_list(self):
        for i in reversed(range(self.cards_layout.count())):
            w = self.cards_layout.itemAt(i).widget()
            if w: w.setParent(None)
        self.card_widgets.clear()

        sorted_servers = sorted(self.servers, key=lambda s: not s.get("fav", False))

        grouped = {}
        for srv in sorted_servers:
            g = srv.get("group", "Мои узлы")
            if g not in grouped:
                grouped[g] = {"url": srv.get("sub_url", ""), "nodes": []}
            grouped[g]["nodes"].append(srv)

        for group_name, data in grouped.items():
            nodes = data["nodes"]
            sub_url = data["url"]

            is_expanded = (group_name == "Мои узлы") or (len(grouped) == 1)

            sub_card = SubscriptionGroupCard(group_name, sub_url, len(nodes), is_expanded)
            sub_card.request_update.connect(self._trigger_update_subscription)
            sub_card.request_delete_group.connect(self._trigger_delete_group)
            sub_card.request_rename_group.connect(self._rename_group)

            for srv in nodes:
                real_idx = self.servers.index(srv)
                card = UltimateServerCard(srv, real_idx)

                card.request_test.connect(self.request_run_test.emit)
                card.request_delete.connect(self._delete_server)
                card.request_favorite.connect(self._toggle_favorite)
                card.request_connect.connect(self.fast_connect_requested.emit)
                card.request_rename.connect(self._rename_server)

                sub_card.add_widget(card)
                self.card_widgets[srv["name"]] = card

            self.cards_layout.addWidget(sub_card)

        self.servers_updated.emit(self.servers)

    def _test_all(self):
        for card in self.card_widgets.values():
            card._run_test("get")

    def _find_best(self):
        self._test_all()
        QTimer.singleShot(2500, lambda: self.combo_sort.setCurrentIndex(1))

    def _filter_list(self, text):
        text = text.lower()
        for name, card in self.card_widgets.items():
            card.setVisible(text in name.lower())

    def _sort_list(self):
        pass

    def _toggle_favorite(self, name, state):
        for s in self.servers:
            if s["name"] == name:
                s["fav"] = state
                break
        self.storage.save(self.servers)
        self._refresh_list()

    def _rename_server(self, old_name, new_name):
        if any(s["name"] == new_name for s in self.servers):
            return
        for s in self.servers:
            if s["name"] == old_name:
                s["name"] = new_name
                break
        self.storage.save(self.servers)
        self._refresh_list()

    def _delete_server(self, name):
        self.servers = [s for s in self.servers if s["name"] != name]
        self.storage.save(self.servers)
        self._refresh_list()

    def update_test_result_ui(self, test_type, server_name, ms):
        if server_name in self.card_widgets:
            self.card_widgets[server_name].update_test_result(test_type, ms)

class StatsView(QWidget):
    def __init__(self):
        super().__init__()
        layout = QVBoxLayout(self)
        layout.setContentsMargins(40, 20, 40, 40)
        layout.setSpacing(20)

        title = QLabel("Статистика сессии")
        title.setStyleSheet("font-size: 24px; font-weight: 900; color: #FFF;")
        layout.addWidget(title)

        self.speed_graph = SpeedGraph()
        layout.addWidget(self.speed_graph)

        grid = QVBoxLayout()
        row1 = QHBoxLayout()
        row2 = QHBoxLayout()

        self.val_dl_speed, self.sub_dl = self._create_card(row1, "⬇️ СКОРОСТЬ СКАЧИВАНИЯ", "0 B/s", "Оранжевый график")
        self.val_ul_speed, self.sub_ul = self._create_card(row1, "⬆️ СКОРОСТЬ ОТДАЧИ", "0 B/s", "Зеленый график")

        self.sub_dl.setStyleSheet("color: #FF6B00; font-size: 12px; font-weight: bold;")
        self.sub_ul.setStyleSheet("color: #10B981; font-size: 12px; font-weight: bold;")

        self.val_vpn_tot, self.sub_vpn = self._create_card(row2, "🛡️ ЗАЩИЩЕННЫЙ ТРАФИК (VPN)", "0 B",
                                                           "0 B получено / 0 B отдано")
        self.val_sys_tot, self.sub_sys = self._create_card(row2, "🌐 ПРЯМОЙ ТРАФИК (SPLIT)", "0 B", "Трафик мимо VPN")

        grid.addLayout(row1)
        grid.addLayout(row2)
        layout.addLayout(grid)
        layout.addStretch()

        self.timer = QTimer()
        self.timer.timeout.connect(self._update_stats)

        self.tun_name = None
        self.is_tracking = False

        self.start_tun_bytes = (0, 0)
        self.start_sys_bytes = (0, 0)
        self.last_tun_bytes = (0, 0)

    def _create_card(self, parent_layout, title_text, val_text, sub_text):
        card = QFrame()
        card.setObjectName("StatsCard")
        card.setGraphicsEffect(create_shadow(20, 5))
        c_layout = QVBoxLayout(card)
        c_layout.setContentsMargins(25, 25, 25, 25)

        lbl_title = QLabel(title_text)
        lbl_title.setObjectName("StatsTitle")

        lbl_val = QLabel(val_text)
        lbl_val.setObjectName("StatsValue")

        lbl_sub = QLabel(sub_text)
        lbl_sub.setObjectName("StatsSub")
        if not sub_text: lbl_sub.hide()

        c_layout.addWidget(lbl_title)
        c_layout.addWidget(lbl_val)
        c_layout.addWidget(lbl_sub)
        parent_layout.addWidget(card)

        return lbl_val, lbl_sub

    def format_bytes(self, size):
        power = 2 ** 10
        n = 0
        power_labels = {0: 'B', 1: 'KB', 2: 'MB', 3: 'GB', 4: 'TB'}
        while size > power:
            size /= power
            n += 1
        return f"{size:.2f} {power_labels[n]}"

    def start_tracking(self, tun_interface_name):
        self.tun_name = tun_interface_name
        self.is_tracking = True
        self.speed_graph.clear()

        stats = psutil.net_io_counters(pernic=True)
        if self.tun_name and self.tun_name in stats:
            self.start_tun_bytes = (stats[self.tun_name].bytes_recv, stats[self.tun_name].bytes_sent)
            self.last_tun_bytes = self.start_tun_bytes

        tot_recv, tot_sent = 0, 0
        for name, stat in stats.items():
            if "Loopback" not in name and "lo" not in name.lower():
                tot_recv += stat.bytes_recv;
                tot_sent += stat.bytes_sent
        self.start_sys_bytes = (tot_recv, tot_sent)

        self.timer.start(1000)

    def stop_tracking(self):
        self.is_tracking = False
        self.timer.stop()
        self.speed_graph.clear()
        self.val_dl_speed.setText("0 B/s")
        self.val_ul_speed.setText("0 B/s")

    def _update_stats(self):
        if not self.is_tracking or not self.tun_name: return
        stats = psutil.net_io_counters(pernic=True)

        if self.tun_name in stats:
            tun = stats[self.tun_name]
            cur_tun_recv, cur_tun_sent = tun.bytes_recv, tun.bytes_sent

            dl_speed = cur_tun_recv - self.last_tun_bytes[0]
            ul_speed = cur_tun_sent - self.last_tun_bytes[1]
            self.last_tun_bytes = (cur_tun_recv, cur_tun_sent)

            self.val_dl_speed.setText(f"{self.format_bytes(dl_speed)}/s")
            self.val_ul_speed.setText(f"{self.format_bytes(ul_speed)}/s")

            self.speed_graph.update_data(dl_speed, ul_speed)

            tot_tun_recv = cur_tun_recv - self.start_tun_bytes[0]
            tot_tun_sent = cur_tun_sent - self.start_tun_bytes[1]

            self.val_vpn_tot.setText(self.format_bytes(tot_tun_recv + tot_tun_sent))
            self.sub_vpn.setText(
                f"{self.format_bytes(tot_tun_recv)} получено / {self.format_bytes(tot_tun_sent)} отдано")

            settings = QSettings("ForgeFox", "VPNClient")
            is_split_on = settings.value("split_enabled", False, type=bool)

            if not is_split_on:
                direct_recv = 0
                direct_sent = 0
            else:
                tot_sys_recv, tot_sys_sent = 0, 0
                for name, stat in stats.items():
                    if "Loopback" not in name and "lo" not in name.lower():
                        tot_sys_recv += stat.bytes_recv
                        tot_sys_sent += stat.bytes_sent

                sys_recv_diff = tot_sys_recv - self.start_sys_bytes[0]
                sys_sent_diff = tot_sys_sent - self.start_sys_bytes[1]

                overhead_recv = int(tot_tun_recv * 0.03)
                overhead_sent = int(tot_tun_sent * 0.03)

                direct_recv = max(0, sys_recv_diff - tot_tun_recv - overhead_recv)
                direct_sent = max(0, sys_sent_diff - tot_tun_sent - overhead_sent)

            self.val_sys_tot.setText(self.format_bytes(direct_recv + direct_sent))
            if direct_recv + direct_sent > 1024:
                self.sub_sys.setStyleSheet("color: #F59E0B;")
                self.sub_sys.setText("Работает Split Tunneling")
            else:
                self.sub_sys.setStyleSheet("color: #71717A;")
                self.sub_sys.setText("Весь трафик в туннеле")

class TrafficView(QWidget):
    def __init__(self):
        super().__init__()
        layout = QVBoxLayout(self)
        layout.setContentsMargins(40, 20, 40, 40)
        layout.setSpacing(20)

        title = QLabel("Монитор соединений")
        title.setStyleSheet("font-size: 24px; font-weight: 900; color: #FFF;")
        layout.addWidget(title)

        columns_layout = QHBoxLayout()
        columns_layout.setSpacing(15)

        self.list_vpn = self._create_column(columns_layout, "🛡️ VPN (Proxy)", "#FF6B00")
        self.list_direct = self._create_column(columns_layout, "🟢 DIRECT (Split)", "#10B981")
        self.list_block = self._create_column(columns_layout, "🚫 BLOCK (AdBlock)", "#EF4444")

        layout.addLayout(columns_layout)

    def _create_column(self, parent_layout, title_text, color):
        col = QFrame()
        col.setObjectName("CyberPanel")
        col.setStyleSheet(f"QFrame#CyberPanel {{ border-top: 3px solid {color}; }}")
        c_layout = QVBoxLayout(col)
        c_layout.setContentsMargins(15, 15, 15, 15)

        lbl = QLabel(title_text)
        lbl.setStyleSheet("font-size: 14px; font-weight: bold; color: #FFF; margin-bottom: 5px;")
        c_layout.addWidget(lbl)

        list_widget = QListWidget()
        list_widget.setStyleSheet("""
            QListWidget { background: transparent; border: none; outline: none; }
            QListWidget::item { color: #A1A1AA; padding: 5px; font-size: 12px; border-bottom: 1px solid #27272A; }
            QListWidget::item:hover { color: #FFF; background: #27272A; border-radius: 4px; }
        """)

        list_widget.setHorizontalScrollBarPolicy(Qt.ScrollBarPolicy.ScrollBarAsNeeded)
        list_widget.setWordWrap(False)

        c_layout.addWidget(list_widget)
        parent_layout.addWidget(col)
        return list_widget

    def add_record(self, category, destination):
        import datetime
        time_str = datetime.datetime.now().strftime("%H:%M:%S")
        item_text = f"[{time_str}] {destination}"

        target_list = None
        if category == 'proxy':
            target_list = self.list_vpn
        elif category == 'direct':
            target_list = self.list_direct
        elif category == 'block':
            target_list = self.list_block

        if target_list is not None:
            if target_list.count() > 0 and target_list.item(0).text().endswith(destination):
                return

            target_list.insertItem(0, item_text)
            if target_list.count() > 100:
                target_list.takeItem(100)


class LogsView(QWidget):
    def __init__(self):
        super().__init__()
        layout = QVBoxLayout(self)

        self.log_area = QTextEdit()
        self.log_area.setObjectName("Terminal")
        self.log_area.setReadOnly(True)
        self.log_area.document().setMaximumBlockCount(1000)

        self.log_area.append(">>> System Booting...")
        layout.addWidget(self.log_area)

    def log(self, txt):
        self.log_area.append(txt)

class SettingsView(QWidget):
    settings_updated = pyqtSignal()

    def __init__(self):
        super().__init__()
        self.settings = QSettings("ForgeFox", "VPNClient")

        layout = QVBoxLayout(self)
        layout.setContentsMargins(40, 20, 40, 40)
        layout.setSpacing(10)

        self.btn_proxy_header = QPushButton("⚙️ Настройки Proxy")
        self.btn_proxy_header.setObjectName("PrimaryBtn")
        self.btn_proxy_header.setFixedHeight(50)
        self.btn_proxy_header.setCursor(QCursor(Qt.CursorShape.PointingHandCursor))
        layout.addWidget(self.btn_proxy_header)

        self.proxy_panel = QFrame()
        self.proxy_panel.setObjectName("CyberPanel")
        self.proxy_panel.setMaximumHeight(0)
        p_layout = QVBoxLayout(self.proxy_panel)

        self.inp_user = QLineEdit()
        self.inp_user.setText(self.settings.value("proxy_user", "Fox"))
        p_layout.addWidget(QLabel("Пользователь (Proxy):"))
        p_layout.addWidget(self.inp_user)

        self.inp_pass = QLineEdit()
        self.inp_pass.setText(self.settings.value("proxy_pass", "Forge"))
        p_layout.addWidget(QLabel("Пароль (Proxy):"))
        p_layout.addWidget(self.inp_pass)

        self.inp_port = QLineEdit()
        self.inp_port.setText(str(self.settings.value("proxy_port", 1080)))
        p_layout.addWidget(QLabel("Порт (Proxy):"))
        p_layout.addWidget(self.inp_port)

        btn_save_proxy = QPushButton("Сохранить Proxy")
        btn_save_proxy.setObjectName("PrimaryBtn")
        btn_save_proxy.clicked.connect(self._save_proxy)
        p_layout.addWidget(btn_save_proxy)

        layout.addWidget(self.proxy_panel)

        self.btn_adblock_header = QPushButton("🚫 Настройки AdBlock")
        self.btn_adblock_header.setObjectName("PrimaryBtn")
        self.btn_adblock_header.setFixedHeight(50)
        self.btn_adblock_header.setCursor(QCursor(Qt.CursorShape.PointingHandCursor))
        layout.addWidget(self.btn_adblock_header)

        self.adblock_panel = QFrame()
        self.adblock_panel.setObjectName("CyberPanel")
        self.adblock_panel.setMaximumHeight(0)
        ab_layout = QVBoxLayout(self.adblock_panel)

        self.inp_adblock_url = QLineEdit()
        default_url = "https://raw.githubusercontent.com/Dreista/sing-box-rule-set-cn/rule-set/filter.txt.srs"
        self.inp_adblock_url.setText(self.settings.value("adblock_url", default_url))
        ab_layout.addWidget(QLabel("Ссылка на базу AdBlock (.srs):"))
        ab_layout.addWidget(self.inp_adblock_url)

        btn_reset_adblock = QPushButton("Вернуть стандартную базу")
        btn_reset_adblock.setObjectName("GhostBtn")
        btn_reset_adblock.setCursor(QCursor(Qt.CursorShape.PointingHandCursor))
        btn_reset_adblock.clicked.connect(lambda: self.inp_adblock_url.setText(default_url))
        ab_layout.addWidget(btn_reset_adblock)

        btn_save_adblock = QPushButton("Сохранить AdBlock")
        btn_save_adblock.setObjectName("PrimaryBtn")
        btn_save_adblock.clicked.connect(self._save_adblock)
        ab_layout.addWidget(btn_save_adblock)

        layout.addWidget(self.adblock_panel)

        self.btn_mh_header = QPushButton("🔗 Настройки Multi-Hop (Double VPN)")
        self.btn_mh_header.setObjectName("PrimaryBtn")
        self.btn_mh_header.setFixedHeight(50)
        self.btn_mh_header.setCursor(QCursor(Qt.CursorShape.PointingHandCursor))
        layout.addWidget(self.btn_mh_header)

        self.mh_panel = QFrame()
        self.mh_panel.setObjectName("CyberPanel")
        self.mh_panel.setMaximumHeight(0)
        mh_layout = QVBoxLayout(self.mh_panel)

        desc_mh = QLabel(
            "Multi-Hop пускает ваш трафик через два сервера подряд. Выберите 'Входной узел' здесь, а 'Выходной' на Главной. Это сильно повышает анонимность, но снижает скорость.")
        desc_mh.setWordWrap(True)
        desc_mh.setStyleSheet("color: #A1A1AA; font-size: 12px; margin-bottom: 5px; line-height: 1.4;")
        mh_layout.addWidget(desc_mh)

        row_mh = QHBoxLayout()
        row_mh.addWidget(QLabel("Включить Double VPN:"))
        self.toggle_mh = AnimatedToggle(checked=self.settings.value("multihop_enabled", False, type=bool))
        self.toggle_mh.toggled.connect(self._save_mh_toggle)
        row_mh.addWidget(self.toggle_mh)
        row_mh.addStretch()
        mh_layout.addLayout(row_mh)

        mh_layout.addWidget(QLabel("Входной узел (первый в цепочке):"))
        self.combo_mh = QComboBox()
        self.combo_mh.setObjectName("ServerDropdown")
        self.combo_mh.currentIndexChanged.connect(self._save_mh_node)
        mh_layout.addWidget(self.combo_mh)
        layout.addWidget(self.mh_panel)

        self.btn_obf_header = QPushButton("📻 Настройки Маскировки (Белый шум)")
        self.btn_obf_header.setObjectName("PrimaryBtn")
        self.btn_obf_header.setFixedHeight(50)
        self.btn_obf_header.setCursor(QCursor(Qt.CursorShape.PointingHandCursor))
        layout.addWidget(self.btn_obf_header)

        self.obf_panel = QFrame()
        self.obf_panel.setObjectName("CyberPanel")
        self.obf_panel.setMaximumHeight(0)
        obf_layout = QVBoxLayout(self.obf_panel)

        desc_lbl = QLabel("Генерирует случайный HTTP-трафик в обход VPN, путая DPI-системы провайдера.")
        desc_lbl.setWordWrap(True)
        desc_lbl.setStyleSheet("color: #A1A1AA; font-size: 12px; margin-bottom: 10px;")
        obf_layout.addWidget(desc_lbl)

        self.inp_obf_sites = QLineEdit()
        default_sites = "wikipedia.org, amazon.com, weather.com, apple.com, microsoft.com, github.com"
        self.inp_obf_sites.setText(self.settings.value("obf_sites", default_sites))
        obf_layout.addWidget(QLabel("Сайты для шума (через запятую):"))
        obf_layout.addWidget(self.inp_obf_sites)

        btn_save_obf = QPushButton("Сохранить сайты маскировки")
        btn_save_obf.setObjectName("PrimaryBtn")
        btn_save_obf.clicked.connect(self._save_obf)
        obf_layout.addWidget(btn_save_obf)

        layout.addWidget(self.obf_panel)
        layout.addStretch()

        self.anim_proxy = QPropertyAnimation(self.proxy_panel, b"maximumHeight");
        self.anim_proxy.setDuration(300);
        self.anim_proxy.setEasingCurve(QEasingCurve.Type.InOutQuad)
        self.anim_adblock = QPropertyAnimation(self.adblock_panel, b"maximumHeight");
        self.anim_adblock.setDuration(300);
        self.anim_adblock.setEasingCurve(QEasingCurve.Type.InOutQuad)
        self.anim_obf = QPropertyAnimation(self.obf_panel, b"maximumHeight");
        self.anim_obf.setDuration(300);
        self.anim_obf.setEasingCurve(QEasingCurve.Type.InOutQuad)
        self.anim_mh = QPropertyAnimation(self.mh_panel, b"maximumHeight");
        self.anim_mh.setDuration(300);
        self.anim_mh.setEasingCurve(QEasingCurve.Type.InOutQuad)

        self.proxy_expanded = False;
        self.adblock_expanded = False;
        self.obf_expanded = False;
        self.mh_expanded = False

        self.btn_proxy_header.clicked.connect(lambda: self._toggle_panel("proxy"))
        self.btn_adblock_header.clicked.connect(lambda: self._toggle_panel("adblock"))
        self.btn_mh_header.clicked.connect(lambda: self._toggle_panel("mh"))
        self.btn_obf_header.clicked.connect(lambda: self._toggle_panel("obf"))

    def update_server_list(self, servers):
        self.combo_mh.blockSignals(True)
        self.combo_mh.clear()
        saved_node = self.settings.value("multihop_entry_node", "")
        idx_to_set = 0
        for i, s in enumerate(servers):
            self.combo_mh.addItem(s["name"])
            if s["name"] == saved_node:
                idx_to_set = i
        if servers:
            self.combo_mh.setCurrentIndex(idx_to_set)
        self.combo_mh.blockSignals(False)

    def _save_mh_toggle(self, state):
        self.settings.setValue("multihop_enabled", state)
        self.settings.sync()

    def _save_mh_node(self):
        self.settings.setValue("multihop_entry_node", self.combo_mh.currentText())
        self.settings.sync()

    def _toggle_panel(self, panel_name):
        panels = {
            "proxy": (self.anim_proxy, self.proxy_expanded, 320),
            "adblock": (self.anim_adblock, self.adblock_expanded, 220),
            "mh": (self.anim_mh, self.mh_expanded, 230),
            "obf": (self.anim_obf, self.obf_expanded, 250)
        }
        anim, is_expanded, height = panels[panel_name]

        if not is_expanded:
            anim.setStartValue(0);
            anim.setEndValue(height)
        else:
            anim.setStartValue(height);
            anim.setEndValue(0)

        anim.start()

        if panel_name == "proxy":
            self.proxy_expanded = not self.proxy_expanded
        elif panel_name == "adblock":
            self.adblock_expanded = not self.adblock_expanded
        elif panel_name == "mh":
            self.mh_expanded = not self.mh_expanded
        elif panel_name == "obf":
            self.obf_expanded = not self.obf_expanded

    def _save_obf(self):
        self.settings.setValue("obf_sites", self.inp_obf_sites.text())
        self.settings.sync();
        self.settings_updated.emit()

    def _save_proxy(self):
        self.settings.setValue("proxy_user", self.inp_user.text())
        self.settings.setValue("proxy_pass", self.inp_pass.text())
        self.settings.setValue("proxy_port", self.inp_port.text())
        self.settings.sync();
        self.settings_updated.emit()

    def _save_adblock(self):
        self.settings.setValue("adblock_url", self.inp_adblock_url.text())
        self.settings.sync();
        self.settings_updated.emit()

class SplitTunnelView(QWidget):
    def __init__(self):
        super().__init__()
        self.settings = QSettings("ForgeFox", "VPNClient")
        layout = QVBoxLayout(self)
        layout.setContentsMargins(40, 20, 40, 40)
        layout.setSpacing(15)

        title = QLabel("Исключения (Split-Tunneling)")
        title.setStyleSheet("font-size: 20px; font-weight: 900; color: #FFF;")
        layout.addWidget(title)

        self.combo_mode = QComboBox()
        self.combo_mode.setObjectName("ServerDropdown")
        self.combo_mode.setCursor(QCursor(Qt.CursorShape.PointingHandCursor))
        self.combo_mode.addItems([
            "🟢 ИСКЛЮЧИТЬ (Всё в VPN, кроме списка ниже)",
            "🟣 ТОЛЬКО ОНИ (Всё напрямую, список ниже — в VPN)"
        ])
        self.combo_mode.currentIndexChanged.connect(self._save_data)
        layout.addWidget(self.combo_mode)

        switch_layout = QHBoxLayout()
        self.btn_sites = QPushButton("САЙТЫ / IP")
        self.btn_apps = QPushButton("ПРИЛОЖЕНИЯ")
        self.btn_sites.setCursor(QCursor(Qt.CursorShape.PointingHandCursor))
        self.btn_apps.setCursor(QCursor(Qt.CursorShape.PointingHandCursor))
        switch_layout.addWidget(self.btn_sites)
        switch_layout.addWidget(self.btn_apps)
        layout.addLayout(switch_layout)

        self.stack = QStackedWidget()
        layout.addWidget(self.stack)


        self.page_sites = QFrame()
        self.page_sites.setObjectName("CyberPanel")
        s_layout = QVBoxLayout(self.page_sites)

        self.btn_presets = QPushButton("✨ Готовые пресеты ⌄")
        self.btn_presets.setObjectName("GhostBtn")
        self.btn_presets.setCursor(QCursor(Qt.CursorShape.PointingHandCursor))
        s_layout.addWidget(self.btn_presets)

        self.presets_panel = QFrame()
        self.presets_panel.setMaximumHeight(0)
        self.presets_panel.setStyleSheet(
            "QFrame { background: #09090B; border-radius: 8px; border: 1px solid #27272A; }")
        p_layout = QVBoxLayout(self.presets_panel)
        p_layout.setContentsMargins(10, 10, 10, 10)
        p_layout.setSpacing(5)

        btn_ru = QPushButton("🇷🇺 Зоны RU (.ru, .рф, .su)")
        btn_yt = QPushButton("🔴 YouTube (Видео + Картинки)")
        btn_tg = QPushButton("✈️ Telegram (Web + CDN + Видео)")
        btn_ig = QPushButton("📸 Instagram (Сайт + Фото + Reels)")

        for btn in [btn_ru, btn_yt, btn_tg, btn_ig]:
            btn.setObjectName("TestBtn")
            btn.setCursor(QCursor(Qt.CursorShape.PointingHandCursor))
            p_layout.addWidget(btn)

        s_layout.addWidget(self.presets_panel)

        self.anim_presets = QPropertyAnimation(self.presets_panel, b"maximumHeight")
        self.anim_presets.setDuration(250)
        self.anim_presets.setEasingCurve(QEasingCurve.Type.InOutQuad)
        self.presets_expanded = False
        self.btn_presets.clicked.connect(self._toggle_presets)

        btn_ru.clicked.connect(lambda: self._add_multiple([".ru", ".рф", ".su"], is_app=False))
        btn_yt.clicked.connect(lambda: self._add_multiple(
            ["youtube.com", "googlevideo.com", "ytimg.com", "youtu.be", "youtubei.googleapis.com", "ggpht.com"],
            is_app=False))
        btn_tg.clicked.connect(
            lambda: self._add_multiple(["telegram.org", "telegram-cdn.org", "t.me", "web.telegram.org"], is_app=False))
        btn_ig.clicked.connect(lambda: self._add_multiple(["instagram.com", "cdninstagram.com", "ig.me"], is_app=False))

        input_s_layout = QHBoxLayout()
        self.inp_domain = QLineEdit()
        self.inp_domain.setPlaceholderText("Например: vk.com, .ru или 1.1.1.1")
        btn_add_domain = QPushButton("Добавить")
        btn_add_domain.setObjectName("PrimaryBtn")
        btn_add_domain.setCursor(QCursor(Qt.CursorShape.PointingHandCursor))
        btn_add_domain.clicked.connect(self._add_domain)
        input_s_layout.addWidget(self.inp_domain)
        input_s_layout.addWidget(btn_add_domain)
        s_layout.addLayout(input_s_layout)

        self.list_domains = QListWidget()
        self.list_domains.setStyleSheet(
            "QListWidget { background: #09090B; border: 1px solid #27272A; border-radius: 8px; padding: 10px; } QListWidget::item { color: #A1A1AA; font-size: 14px; padding: 5px; } QListWidget::item:selected { color: #FFF; background: #27272A; }")
        s_layout.addWidget(self.list_domains)

        btn_del_domain = QPushButton("Удалить выбранное")
        btn_del_domain.setObjectName("DangerBtn")
        btn_del_domain.setCursor(QCursor(Qt.CursorShape.PointingHandCursor))
        btn_del_domain.clicked.connect(self._del_domain)
        s_layout.addWidget(btn_del_domain)
        self.stack.addWidget(self.page_sites)

        self.page_apps = QFrame()
        self.page_apps.setObjectName("CyberPanel")
        a_layout = QVBoxLayout(self.page_apps)

        self.btn_app_presets = QPushButton("✨ Готовые пресеты ⌄")
        self.btn_app_presets.setObjectName("GhostBtn")
        self.btn_app_presets.setCursor(QCursor(Qt.CursorShape.PointingHandCursor))
        a_layout.addWidget(self.btn_app_presets)

        self.app_presets_panel = QFrame()
        self.app_presets_panel.setMaximumHeight(0)
        self.app_presets_panel.setStyleSheet(
            "QFrame { background: #09090B; border-radius: 8px; border: 1px solid #27272A; }")
        ap_layout = QVBoxLayout(self.app_presets_panel)
        ap_layout.setContentsMargins(10, 10, 10, 10)
        ap_layout.setSpacing(5)

        btn_app_ds = QPushButton("🎮 Discord (discord.exe)")
        btn_app_tg = QPushButton("✈️ Telegram (telegram.exe, tg.exe)")
        btn_app_browsers = QPushButton("🌐 Браузеры (Chrome, Edge, Firefox, Yandex)")
        btn_app_steam = QPushButton("🕹️ Steam (steam.exe, steamwebhelper.exe)")

        for btn in [btn_app_ds, btn_app_tg, btn_app_browsers, btn_app_steam]:
            btn.setObjectName("TestBtn")
            btn.setCursor(QCursor(Qt.CursorShape.PointingHandCursor))
            ap_layout.addWidget(btn)

        a_layout.addWidget(self.app_presets_panel)

        self.anim_app_presets = QPropertyAnimation(self.app_presets_panel, b"maximumHeight")
        self.anim_app_presets.setDuration(250)
        self.anim_app_presets.setEasingCurve(QEasingCurve.Type.InOutQuad)
        self.app_presets_expanded = False
        self.btn_app_presets.clicked.connect(self._toggle_app_presets)

        btn_app_ds.clicked.connect(lambda: self._add_multiple(["Discord.exe", "Update.exe"], is_app=True))
        btn_app_tg.clicked.connect(lambda: self._add_multiple(["Telegram.exe", "tg.exe", "telegramdesktop.exe"], is_app=True))
        btn_app_browsers.clicked.connect(lambda: self._add_multiple(["chrome.exe", "msedge.exe", "firefox.exe", "yandex.exe", "opera.exe", "brave.exe"], is_app=True))
        btn_app_steam.clicked.connect(lambda: self._add_multiple(["steam.exe", "steamwebhelper.exe"], is_app=True))

        input_a_layout = QHBoxLayout()
        self.inp_app = QLineEdit()
        self.inp_app.setPlaceholderText("Например: discord.exe")
        btn_add_app = QPushButton("Добавить")
        btn_add_app.setObjectName("PrimaryBtn")
        btn_add_app.setCursor(QCursor(Qt.CursorShape.PointingHandCursor))
        btn_add_app.clicked.connect(self._add_app)
        input_a_layout.addWidget(self.inp_app)
        input_a_layout.addWidget(btn_add_app)
        a_layout.addLayout(input_a_layout)

        self.list_apps = QListWidget()
        self.list_apps.setStyleSheet(
            "QListWidget { background: #09090B; border: 1px solid #27272A; border-radius: 8px; padding: 10px; } QListWidget::item { color: #A1A1AA; font-size: 14px; padding: 5px; } QListWidget::item:selected { color: #FFF; background: #27272A; }")
        a_layout.addWidget(self.list_apps)

        btn_del_app = QPushButton("Удалить выбранное")
        btn_del_app.setObjectName("DangerBtn")
        btn_del_app.setCursor(QCursor(Qt.CursorShape.PointingHandCursor))
        btn_del_app.clicked.connect(self._del_app)
        a_layout.addWidget(btn_del_app)
        self.stack.addWidget(self.page_apps)

        self.btn_sites.clicked.connect(lambda: self._set_tab(0))
        self.btn_apps.clicked.connect(lambda: self._set_tab(1))
        self._set_tab(0)
        self._load_data()


    def _toggle_presets(self):
        if not self.presets_expanded:
            self.anim_presets.setStartValue(0);
            self.anim_presets.setEndValue(160)
            self.btn_presets.setText("✨ Готовые пресеты ⌃")
        else:
            self.anim_presets.setStartValue(160);
            self.anim_presets.setEndValue(0)
            self.btn_presets.setText("✨ Готовые пресеты ⌄")
        self.anim_presets.start()
        self.presets_expanded = not self.presets_expanded

    def _toggle_app_presets(self):
        if not self.app_presets_expanded:
            self.anim_app_presets.setStartValue(0);
            self.anim_app_presets.setEndValue(160)
            self.btn_app_presets.setText("✨ Готовые пресеты ⌃")
        else:
            self.anim_app_presets.setStartValue(160);
            self.anim_app_presets.setEndValue(0)
            self.btn_app_presets.setText("✨ Готовые пресеты ⌄")
        self.anim_app_presets.start()
        self.app_presets_expanded = not self.app_presets_expanded

    def _set_tab(self, idx):
        self.stack.setCurrentIndex(idx)
        self.btn_sites.setObjectName("ModeBtnActive" if idx == 0 else "ModeBtnInactive")
        self.btn_apps.setObjectName("ModeBtnActive" if idx == 1 else "ModeBtnInactive")
        self.btn_sites.style().unpolish(self.btn_sites);
        self.btn_sites.style().polish(self.btn_sites)
        self.btn_apps.style().unpolish(self.btn_apps);
        self.btn_apps.style().polish(self.btn_apps)

    def _load_data(self):
        self.combo_mode.blockSignals(True)
        try:
            mode = int(self.settings.value("split_mode", 0))
        except:
            mode = 0
        self.combo_mode.setCurrentIndex(mode)

        raw_domains = self.settings.value("bypass_domains", [])
        raw_apps = self.settings.value("bypass_apps", [])

        self.list_domains.clear()
        self.list_apps.clear()

        def clean_data(raw_data):
            if isinstance(raw_data, str):
                return [raw_data]

            clean_list = []
            for item in raw_data:
                if isinstance(item, str):
                    clean_list.append(item)
                elif isinstance(item, dict) and "target" in item:
                    clean_list.append(item["target"])
            return clean_list

        domains = clean_data(raw_domains)
        apps = clean_data(raw_apps)

        if domains: self.list_domains.addItems(domains)
        if apps: self.list_apps.addItems(apps)

        self.combo_mode.blockSignals(False)

    def _save_data(self):
        self.settings.setValue("split_mode", self.combo_mode.currentIndex())
        domains = [self.list_domains.item(i).text() for i in range(self.list_domains.count())]
        apps = [self.list_apps.item(i).text() for i in range(self.list_apps.count())]
        self.settings.setValue("bypass_domains", domains)
        self.settings.setValue("bypass_apps", apps)
        self.settings.sync()

    def _add_multiple(self, presets, is_app=False):
        target_list = self.list_apps if is_app else self.list_domains
        existing = [target_list.item(i).text() for i in range(target_list.count())]
        for p in presets:
            if p not in existing:
                target_list.addItem(p)
        self._save_data()

        if is_app:
            self._toggle_app_presets()
        else:
            self._toggle_presets()

    def _add_domain(self):
        txt = self.inp_domain.text().strip().lower()
        if txt:
            txt = txt.replace("https://", "").replace("http://", "")
            if txt.startswith("www."): txt = txt[4:]
            txt = txt.split('/')[0]

            if txt:
                existing = [self.list_domains.item(i).text() for i in range(self.list_domains.count())]
                if txt not in existing:
                    self.list_domains.addItem(txt)
                self.inp_domain.clear()
                self._save_data()

    def _del_domain(self):
        for item in self.list_domains.selectedItems():
            self.list_domains.takeItem(self.list_domains.row(item))
        self._save_data()

    def _add_app(self):
        txt = self.inp_app.text().strip()
        if txt and not txt.lower().endswith(".exe"): txt += ".exe"
        if txt:
            existing = [self.list_apps.item(i).text() for i in range(self.list_apps.count())]
            if txt not in existing:
                self.list_apps.addItem(txt)
            self.inp_app.clear()
            self._save_data()

    def _del_app(self):
        for item in self.list_apps.selectedItems():
            self.list_apps.takeItem(self.list_apps.row(item))
        self._save_data()



class CustomTitleBar(QWidget):
    def __init__(self, parent=None):
        super().__init__(parent)
        self.setFixedHeight(35)
        self.dragPos = None
        layout = QHBoxLayout(self)
        layout.setContentsMargins(15, 0, 10, 0)

        self.icon_label = QLabel()
        icon_path = resource_path("static/icon.ico")
        if os.path.exists(icon_path):
            self.icon_label.setPixmap(QIcon(icon_path).pixmap(18, 18))
        layout.addWidget(self.icon_label)

        title = QLabel("ForgeFox VPN")
        title.setStyleSheet("color: #A1A1AA; font-weight: bold; font-size: 13px;")
        layout.addWidget(title)
        layout.addStretch()

        btn_style = "QPushButton { background: transparent; color: #A1A1AA; font-size: 16px; border-radius: 5px; width: 30px; height: 30px; } "

        self.btn_min = QPushButton("—")
        self.btn_min.setStyleSheet(btn_style + "QPushButton:hover { background: #27272A; }")
        self.btn_min.clicked.connect(self.window().fade_out_and_hide)
        layout.addWidget(self.btn_min)

        self.btn_close = QPushButton("✕")
        self.btn_close.setStyleSheet(btn_style + "QPushButton:hover { background: #EF4444; color: white; }")
        self.btn_close.clicked.connect(self.window().close)
        layout.addWidget(self.btn_close)

    def mousePressEvent(self, event):
        if event.button() == Qt.MouseButton.LeftButton:
            self.dragPos = event.globalPosition().toPoint()

    def mouseMoveEvent(self, event):
        if self.dragPos is not None:
            delta = event.globalPosition().toPoint() - self.dragPos
            self.window().move(self.window().pos() + delta)
            self.dragPos = event.globalPosition().toPoint()

class CopyLabel(QLabel):
    def __init__(self, text, copy_text=None):
        super().__init__(text)
        self.original_text = text
        self.copy_text = copy_text if copy_text else text
        self.setCursor(QCursor(Qt.CursorShape.PointingHandCursor))

    def mousePressEvent(self, event):
        if event.button() == Qt.MouseButton.LeftButton:
            QApplication.clipboard().setText(self.copy_text)

            self.setText("Скопировано")

            QTimer.singleShot(1000, self.restore_text)

    def restore_text(self):
        self.setText(self.original_text)

def create_shadow(radius=30, offset=10, color=QColor(0, 0, 0, 150)):
    shadow = QGraphicsDropShadowEffect()
    shadow.setBlurRadius(radius);
    shadow.setColor(color);
    shadow.setOffset(0, offset)
    return shadow


class SpeedGraph(QWidget):
    def __init__(self):
        super().__init__()
        self.setFixedHeight(180)
        self.max_points = 60
        self.dl_data = [0] * self.max_points
        self.ul_data = [0] * self.max_points

        self.color_dl = QColor("#FF6B00")
        self.color_ul = QColor("#10B981")

    def update_data(self, dl, ul):
        self.dl_data.pop(0)
        self.dl_data.append(dl)
        self.ul_data.pop(0)
        self.ul_data.append(ul)
        self.update()

    def clear(self):
        self.dl_data = [0] * self.max_points
        self.ul_data = [0] * self.max_points
        self.update()

    def paintEvent(self, event):
        p = QPainter(self)
        p.setRenderHint(QPainter.RenderHint.Antialiasing)
        w = self.width()
        h = self.height()

        p.setPen(Qt.PenStyle.NoPen)
        p.setBrush(QColor("#18181B"))
        p.drawRoundedRect(0, 0, w, h, 12, 12)
        p.setPen(QPen(QColor("#27272A"), 1))
        p.setBrush(Qt.BrushStyle.NoBrush)
        p.drawRoundedRect(0, 0, w, h, 12, 12)

        p.setPen(QPen(QColor("#27272A"), 1, Qt.PenStyle.DashLine))
        for i in range(1, 4):
            y_line = int(h * i / 4)
            p.drawLine(0, y_line, w, y_line)

        max_val = max(max(self.dl_data), max(self.ul_data), 1024 * 1024)

        def draw_line(data, color):
            path = QPainterPath()
            step = w / (self.max_points - 1)

            for i, val in enumerate(data):
                x = i * step
                y = h - ((val / max_val) * (h - 10)) - 5

                if i == 0:
                    path.moveTo(x, y)
                else:
                    path.lineTo(x, y)

            p.setPen(QPen(color, 2))
            p.setBrush(Qt.BrushStyle.NoBrush)
            p.drawPath(path)

            fill_path = QPainterPath(path)
            fill_path.lineTo(w, h)
            fill_path.lineTo(0, h)
            fill_path.closeSubpath()

            gradient = QLinearGradient(0, 0, 0, h)
            color_top = QColor(color);
            color_top.setAlpha(80)
            color_bot = QColor(color);
            color_bot.setAlpha(0)
            gradient.setColorAt(0, color_top)
            gradient.setColorAt(1, color_bot)

            p.setPen(Qt.PenStyle.NoPen)
            p.setBrush(QBrush(gradient))
            p.drawPath(fill_path)

        draw_line(self.ul_data, self.color_ul)
        draw_line(self.dl_data, self.color_dl)


class WaveVisualizer(QWidget):
    def __init__(self):
        super().__init__()
        self.setFixedHeight(40)
        self.phase = 0
        self.is_active = False
        self.timer = QTimer(self)
        self.timer.timeout.connect(self._update_wave)

    def set_active(self, active):
        self.is_active = active
        if active:
            self.timer.start(16)
        else:
            self.timer.stop(); self.phase = 0; self.update()

    def _update_wave(self):
        self.phase += 0.08
        self.update()

    def paintEvent(self, event):
        if not self.is_active: return
        p = QPainter(self)
        p.setRenderHint(QPainter.RenderHint.Antialiasing)
        width = self.width(); height = self.height(); mid_y = height / 2

        for i in range(3):
            path = QPainterPath()
            path.moveTo(0, mid_y)
            amplitude = 12 if i == 1 else 6
            speed = 0.03 + (i * 0.015)
            offset = self.phase * (1 + i * 0.2)
            for x in range(width):
                y = mid_y + math.sin(x * speed + offset) * amplitude
                path.lineTo(x, y)
            color = QColor("#FF6B00"); color.setAlpha(150 - (i * 50))
            p.setPen(QPen(color, 2)); p.drawPath(path)


class PowerWheel(QWidget):
    clicked = pyqtSignal()

    def __init__(self):
        super().__init__()
        self.setFixedSize(160, 160)
        self.setCursor(QCursor(Qt.CursorShape.PointingHandCursor))
        self.state = "off"
        self.hovered = False
        self.angle = 0
        self.timer = QTimer(self)
        self.timer.timeout.connect(self._rotate)
        self.color_bg = QColor("#27272A")
        self.color_hover = QColor("#3F3F46")
        self.color_accent = QColor("#FF6B00")

    def set_state(self, new_state):
        self.state = new_state
        if self.state == "loading":
            self.timer.start(16)
        else:
            self.timer.stop(); self.angle = 0
        self.update()

    def _rotate(self):
        self.angle = (self.angle + 8) % 360; self.update()

    def enterEvent(self, e):
        self.hovered = True; self.update()

    def leaveEvent(self, e):
        self.hovered = False; self.update()

    def mouseReleaseEvent(self, e):
        if e.button() == Qt.MouseButton.LeftButton:
            if self.state == "loading":
                self.state = "on"
            self.clicked.emit()

    def paintEvent(self, event):
        p = QPainter(self)
        p.setRenderHint(QPainter.RenderHint.Antialiasing)
        rect = self.rect()
        bg = self.color_hover if self.hovered and self.state == "off" else self.color_bg
        p.setBrush(QBrush(bg))
        p.setPen(Qt.PenStyle.NoPen)
        p.drawEllipse(rect.adjusted(10, 10, -10, -10))
        p.setPen(QPen(QColor("#18181B"), 6, Qt.PenStyle.SolidLine, Qt.PenCapStyle.RoundCap))
        p.setBrush(Qt.BrushStyle.NoBrush)
        p.drawEllipse(rect.adjusted(15, 15, -15, -15))

        center = rect.center()
        if self.state == "loading":
            p.setPen(QPen(self.color_accent, 6, Qt.PenStyle.SolidLine, Qt.PenCapStyle.RoundCap))
            p.drawArc(QRectF(15, 15, rect.width() - 30, rect.height() - 30), -self.angle * 16, 120 * 16)
            p.setPen(QColor("#A1A1AA"))
            p.setFont(QFont("Segoe UI", 12, QFont.Weight.Bold))
            p.drawText(rect, Qt.AlignmentFlag.AlignCenter, "CONNECTING")
        elif self.state == "on":
            p.setPen(QPen(self.color_accent, 6, Qt.PenStyle.SolidLine, Qt.PenCapStyle.RoundCap))
            p.drawEllipse(rect.adjusted(15, 15, -15, -15))
            p.setPen(self.color_accent)
            p.setFont(QFont("Segoe UI", 20, QFont.Weight.Black))
            p.drawText(rect, Qt.AlignmentFlag.AlignCenter, "ON")
        else:
            p.setPen(QPen(QColor("#71717A"), 4, Qt.PenStyle.SolidLine, Qt.PenCapStyle.RoundCap))
            p.drawArc(QRectF(55, 55, 50, 50), -60 * 16, 300 * 16)
            p.drawLine(center.x(), 45, center.x(), 80)

class AnimatedToggle(QWidget):
    toggled = pyqtSignal(bool)

    def __init__(self, checked=True):
        super().__init__()
        self.setFixedSize(46, 24)
        self.setCursor(Qt.CursorShape.PointingHandCursor)
        self._is_checked = checked
        self._position = 24 if checked else 3
        self.anim = QPropertyAnimation(self, b"position")
        self.anim.setEasingCurve(QEasingCurve.Type.InOutBack)
        self.anim.setDuration(250)

    @pyqtProperty(float)
    def position(self):
        return self._position

    @position.setter
    def position(self, pos):
        self._position = pos
        self.update()

    def mouseReleaseEvent(self, event):
        if event.button() == Qt.MouseButton.LeftButton:
            self._is_checked = not self._is_checked
            self.anim.setStartValue(self._position)
            self.anim.setEndValue(24 if self._is_checked else 3)
            self.anim.start()
            self.toggled.emit(self._is_checked)

    def paintEvent(self, event):
        p = QPainter(self)
        p.setRenderHint(QPainter.RenderHint.Antialiasing)
        p.setPen(Qt.PenStyle.NoPen)

        bg_color = QColor("#FF6B00") if self._is_checked else QColor("#3F3F46")
        p.setBrush(QBrush(bg_color))
        p.drawRoundedRect(0, 0, self.width(), self.height(), 12, 12)

        p.setBrush(QBrush(QColor("#FFFFFF")))
        p.drawEllipse(QRectF(self._position, 3, 18, 18))

class MainWindow(QMainWindow):
    def __init__(self):
        super().__init__()
        self.setWindowTitle("ForgeFoxVPN")
        self.resize(1250, 700)
        self.setMinimumSize(950, 600)
        self.setStyleSheet(STYLE_SHEET)
        self.setWindowFlags(Qt.WindowType.FramelessWindowHint | Qt.WindowType.WindowMinimizeButtonHint)
        self.setAttribute(Qt.WidgetAttribute.WA_TranslucentBackground)

        self.storage = SecureStorage()
        self.settings = QSettings("ForgeFox", "VPNClient")
        self.current_mode = self.settings.value("current_mode", "tunnel")
        self.vpn_thread = None
        self.selected_server_idx = 0
        self.test_workers = []
        self.retry_count = 0
        self.tun_checks = 0
        self.checker_id = 0
        self.tun_ready = False
        self.dns_cache = {}
        self.obfuscator_thread = None

        self._build_ui()
        self._setup_tray()
        self._wire_signals()

        self._setup_ipc()

    def _setup_ipc(self):
        QLocalServer.removeServer("ForgeFoxVPN_IPC")

        self.ipc_server = QLocalServer(self)
        self.ipc_server.newConnection.connect(self._on_ipc_connection)
        self.ipc_server.listen("ForgeFoxVPN_IPC")

    def _on_ipc_connection(self):
        socket = self.ipc_server.nextPendingConnection()
        if socket.waitForReadyRead(500):
            data = socket.readAll().data()
            if data == b"WAKE":
                self.fade_in_and_show()

        socket.disconnectFromServer()
        socket.deleteLater()

    def _setup_tray(self):
        self.tray_icon = QSystemTrayIcon(self)
        if os.path.exists(resource_path("static/icon.ico")):
            self.tray_icon.setIcon(QIcon(resource_path("static/icon.ico")))

        tray_menu = QMenu()
        tray_menu.setStyleSheet(
            "QMenu { background-color: #18181B; color: #E4E4E7; border: 1px solid #27272A; } "
            "QMenu::item { padding: 8px 20px; } "
            "QMenu::item:selected { background-color: #FF6B00; }"
        )

        self.action_toggle_vpn = QAction("Подключить", self)
        self.action_toggle_vpn.triggered.connect(self._toggle_vpn_from_tray)

        show_action = QAction("Развернуть ForgeFox", self)
        show_action.triggered.connect(self.fade_in_and_show)

        quit_action = QAction("Выход", self)
        quit_action.triggered.connect(self.quit_app)

        tray_menu.addAction(self.action_toggle_vpn)
        tray_menu.addSeparator()
        tray_menu.addAction(show_action)
        tray_menu.addAction(quit_action)

        self.tray_icon.setContextMenu(tray_menu)
        self.tray_icon.activated.connect(self._tray_activated)
        self.tray_icon.show()

    def _toggle_vpn_from_tray(self):
        if self.view_home.wheel.state in ["on", "loading"]:
            self.view_home.request_disconnect.emit()
        else:
            self.view_home.request_connect.emit()

    def update_tray_menu(self, is_connected):
        if hasattr(self, 'action_toggle_vpn'):
            self.action_toggle_vpn.setText("Отключить" if is_connected else "Подключить")

    def fade_out_and_hide(self):
        self.anim = QPropertyAnimation(self, b"windowOpacity")
        self.anim.setDuration(200)
        self.anim.setStartValue(1.0)
        self.anim.setEndValue(0.0)
        self.anim.finished.connect(self.hide)
        self.anim.start()

    def fade_in_and_show(self):
        self.setWindowOpacity(0.0)
        self.showNormal()
        self.activateWindow()
        self.raise_()
        self.anim = QPropertyAnimation(self, b"windowOpacity")
        self.anim.setDuration(200)
        self.anim.setStartValue(0.0)
        self.anim.setEndValue(1.0)
        self.anim.start()

    def _tray_activated(self, reason):
        if reason == QSystemTrayIcon.ActivationReason.DoubleClick:
            self.fade_in_and_show()

    def quit_app(self):
        self.checker_id = 0
        if self.vpn_thread:
            self.vpn_thread.is_running = False
            self.vpn_thread.stop()

        CREATE_NO_WINDOW = 0x08000000
        subprocess.run(['taskkill', '/F', '/IM', 'sing-box.exe'],
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                       creationflags=CREATE_NO_WINDOW)
        QApplication.quit()

    def quit_app(self):
        self.checker_id = 0
        if self.vpn_thread:
            self.vpn_thread.is_running = False
            self.vpn_thread.stop()

        CREATE_NO_WINDOW = 0x08000000
        subprocess.run(['taskkill', '/F', '/IM', 'sing-box.exe'],
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                       creationflags=CREATE_NO_WINDOW)
        QApplication.quit()

    def _build_ui(self):
        self.main_frame = QFrame()
        self.main_frame.setObjectName("MainFrame")
        self.main_frame.setStyleSheet(STYLE_SHEET)
        self.setCentralWidget(self.main_frame)

        main_layout = QVBoxLayout(self.main_frame)
        main_layout.setContentsMargins(0, 0, 0, 0)
        main_layout.setSpacing(0)

        self.title_bar = CustomTitleBar(self)
        main_layout.addWidget(self.title_bar)

        content_widget = QWidget()
        content_layout = QHBoxLayout(content_widget)
        content_layout.setContentsMargins(0, 0, 0, 0)
        content_layout.setSpacing(0)

        side_container = QWidget(); side_container.setFixedWidth(240); side_container.setStyleSheet("background-color: #18181B;")
        side_layout = QVBoxLayout(side_container)
        logo = QLabel("🦊 ForgeFoxVPN"); logo.setStyleSheet("color: #FF6B00; font-size: 18px; font-weight: 900; padding: 20px;"); side_layout.addWidget(logo)
        self.sidebar = QListWidget(); self.sidebar.setObjectName("Sidebar")
        for it in ["Главная", "Конфиги", "Исключения", "Статистика", "Трафик", "Настройки", "Системный лог"]: self.sidebar.addItem(it)
        side_layout.addWidget(self.sidebar); content_layout.addWidget(side_container)



        self.stack = QStackedWidget()
        self.view_servers = ServersView(self.storage);
        self.view_home = HomeView(); self.view_servers = ServersView(self.storage); self.view_stats = StatsView(); self.view_traffic = TrafficView(); self.view_logs = LogsView(); self.view_settings = SettingsView(); self.view_split = SplitTunnelView()
        self.stack.addWidget(self.view_home); self.stack.addWidget(self.view_servers); self.stack.addWidget(self.view_split); self.stack.addWidget(self.view_stats); self.stack.addWidget(self.view_traffic); self.stack.addWidget(self.view_settings); self.stack.addWidget(self.view_logs);
        content_layout.addWidget(self.stack)

        main_layout.addWidget(content_widget)
        self.sidebar.setCurrentRow(0)
        self.view_home.update_combo_list(self.view_servers.servers)
        self.view_home.set_mode(self.current_mode)

    def _wire_signals(self):
        self.sidebar.currentRowChanged.connect(self.stack.setCurrentIndex)
        self.view_servers.servers_updated.connect(self.view_home.update_combo_list)
        self.view_home.server_selected.connect(self._sync_selected_server)

        self.view_home.request_connect.connect(self._handle_connect)
        self.view_home.request_disconnect.connect(self._handle_disconnect)
        self.view_home.request_mode.connect(self._set_mode)

        self.view_servers.request_run_test.connect(self._spawn_test_worker)
        self.view_home.request_quick_ping.connect(self._handle_quick_ping)

        self.view_settings.settings_updated.connect(self._apply_new_settings)
        self.view_servers.fast_connect_requested.connect(self._handle_fast_connect)

        self.view_servers.servers_updated.connect(self.view_settings.update_server_list)
        self.view_settings.update_server_list(self.view_servers.servers)

    def _handle_fast_connect(self, idx):
        self.sidebar.setCurrentRow(0)
        self.stack.setCurrentIndex(0)

        if self.selected_server_idx == idx:
            if self.view_home.wheel.state == "on":
                self.view_logs.log(f"[*] Принудительный перезапуск текущего узла...")
                self._handle_disconnect()
                QTimer.singleShot(1000, self._handle_connect)
            elif self.view_home.wheel.state in ["off", "error"]:
                self.view_home._on_wheel_click()
            return

        self.view_home._on_server_selected(idx)

        if self.view_home.wheel.state in ["off", "error"]:
            self.view_home._on_wheel_click()

    def _apply_new_settings(self):
        user = self.settings.value("proxy_user", "Fox")
        pwd = self.settings.value("proxy_pass", "Forge")
        port = self.settings.value("proxy_port", 1080)

        self.view_home.lbl_proxy_host.setText(f"Host: 127.0.0.1 | Port: {port}")
        self.view_home.lbl_proxy_host.copy_text = f"127.0.0.1:{port}"

        self.view_home.lbl_proxy_info.setText(f"User: {user} | Pass: {pwd} | Socks5")
        self.view_home.lbl_proxy_info.copy_text = f"{user}:{pwd}"

        self.view_logs.log("[*] Параметры локального прокси обновлены.")

    def _sync_selected_server(self, idx):
        if getattr(self, "selected_server_idx", None) == idx:
            return

        self.selected_server_idx = idx

        if self.vpn_thread is not None:
            self.view_logs.log("[*] Инициирована горячая смена узла...")

            self.view_home.wheel.set_state("loading")
            self.view_home.lbl_status.setText("СМЕНА УЗЛА...")
            self.view_home.lbl_status.setStyleSheet("color: #FF6B00; font-weight: bold; letter-spacing: 2px;")

            self.checker_id = 0
            try:
                self.vpn_thread.log_signal.disconnect()
            except:
                pass

            self.vpn_thread.stop()
            self.vpn_thread = None

            CREATE_NO_WINDOW = 0x08000000
            subprocess.run(['taskkill', '/F', '/IM', 'sing-box.exe'],
                           stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                           creationflags=CREATE_NO_WINDOW)

            servers = self.view_servers.servers
            if 0 <= idx < len(servers):
                srv = servers[idx]
                self.retry_count = 0
                QTimer.singleShot(1500, lambda: self._start_vpn_process(srv))

    def _spawn_test_worker(self, test_type, server_dict):
        if not hasattr(self, "test_workers"):
            self.test_workers = []

        self.test_workers = [w for w in self.test_workers if w.isRunning()]
        try:
            worker = NetworkTester(test_type, server_dict)
            worker.result_signal.connect(self._on_test_finished)
            self.test_workers.append(worker)
            worker.start()
        except Exception:
            self.view_servers.update_test_result_ui(test_type, server_dict["name"], -1)

    def _on_test_finished(self, test_type, server_name, ms):
        self.view_servers.update_test_result_ui(test_type, server_name, ms)

    def _handle_quick_ping(self):
        servers = self.view_servers.servers
        if not servers or self.selected_server_idx < 0 or self.selected_server_idx >= len(servers): return
        srv = servers[self.selected_server_idx]
        self.view_home.btn_quick_ping.setText("⏳")
        if not hasattr(self, "test_workers"):
            self.test_workers = []

        self.test_workers = [w for w in self.test_workers if w.isRunning()]
        try:
            worker = NetworkTester("ping", srv)
            worker.result_signal.connect(lambda t, n, ms: self.view_home.show_quick_ping(ms))
            self.test_workers.append(worker)
            worker.start()
        except:
            self.view_home.show_quick_ping(-1)

    def _set_mode(self, mode):
        self.current_mode = mode
        self.settings.setValue("current_mode", mode)

    def _handle_connect(self):
        if self.current_mode == "adblock":
            self.retry_count = 0
            self.tun_checks = 0
            self.view_logs.log("[*] Инициализация чистого блокировщика рекламы...")
            QTimer.singleShot(1000, lambda: self._start_vpn_process(None))
            return

        servers = self.view_servers.servers
        if not servers or self.selected_server_idx < 0 or self.selected_server_idx >= len(servers):
            self.view_logs.log("[!] Ошибка: Выберите сервер")
            self.view_home.set_error_state()
            return

        srv = servers[self.selected_server_idx]
        self.retry_count = 0
        self.tun_checks = 0
        self.view_logs.log(f"[*] Инициализация соединения с {srv['name']}...")
        QTimer.singleShot(1000, lambda: self._start_vpn_process(srv))

    def _start_vpn_process(self, srv):
        try:
            def get_list(key):
                val = self.settings.value(key, [])
                if isinstance(val, str): return [val]

                clean_list = []
                for item in (val if val else []):
                    if isinstance(item, str):
                        clean_list.append(item)
                    elif isinstance(item, dict) and "target" in item:
                        clean_list.append(item["target"])
                return clean_list

            px_user = self.settings.value("proxy_user", "Fox")
            px_pass = self.settings.value("proxy_pass", "Forge")
            px_port = self.settings.value("proxy_port", 1080)

            split_enabled = self.settings.value("split_enabled", False, type=bool)
            bypass_domains = get_list("bypass_domains")
            bypass_apps = get_list("bypass_apps")
            split_mode = int(self.settings.value("split_mode", 0))

            adblock_enabled = self.settings.value("adblock_enabled", False, type=bool)
            default_url = "https://raw.githubusercontent.com/Dreista/sing-box-rule-set-cn/rule-set/filter.txt.srs"
            adblock_url = self.settings.value("adblock_url", default_url)

            v = VPNManager.parse_vless(srv["link"]) if self.current_mode != "adblock" else None

            detour_v = None
            if self.settings.value("multihop_enabled", False, type=bool) and self.current_mode != "adblock":
                entry_name = self.settings.value("multihop_entry_node", "")
                entry_srv = next((s for s in self.view_servers.servers if s["name"] == entry_name), None)
                if entry_srv:
                    try:
                        detour_v = VPNManager.parse_vless(entry_srv["link"])
                        self.view_logs.log(f"[*] Multi-Hop активен! Маршрут: {entry_name} ➔ {srv['name']}")
                    except:
                        self.view_logs.log(f"[!] Ошибка парсинга входного узла '{entry_name}'. Multi-Hop отключен.")

            config = VPNManager.build_config(
                v, self.current_mode, px_user, px_pass, px_port,
                bypass_domains, bypass_apps, split_mode, adblock_enabled, adblock_url, split_enabled,
                detour_v=detour_v
            )

            self.vpn_thread = VPNThread(config)
            self.vpn_thread.log_signal.connect(self._process_vpn_logs)
            self.vpn_thread.process_died_signal.connect(self._handle_process_crash)
            self.vpn_thread.start()

            self.view_logs.log(f"[*] Ядро запущено. Split Mode: {'Исключить' if split_mode == 0 else 'Только список'}")
            self._start_tun_checker()

        except Exception as e:
            self.view_logs.log(f"[!] Фатальная ошибка: {e}")
            self.view_home.set_error_state()

    def _process_vpn_logs(self, text):
        self.view_logs.log(text)

        clean_text = re.sub(r'\x1b\[[0-9;]*m', '', text)

        dns_match = re.search(r'dns: exchanged (?:A|AAAA) ([^\s]+)\.? \d+ IN (?:A|AAAA) ([^\s]+)', clean_text)
        if dns_match:
            domain = dns_match.group(1).rstrip('.')
            ip_addr = dns_match.group(2)
            self.dns_cache[ip_addr] = domain

        match = re.search(
            r'outbound/[^\[]+\[(proxy|direct|block)\]: (?:outbound connection to|blocked connection to) ([^\s]+)',
            clean_text)
        if match:
            category = match.group(1)
            raw_dest = match.group(2)

            target = raw_dest
            host_match = re.match(r'^(?:\[([^\]]+)\]|([^:]+))(?::\d+)?$', raw_dest)
            if host_match:
                target = host_match.group(1) or host_match.group(2)

            display_dest = target

            if re.match(r'^[\d\.]+$', target) or ':' in target:
                domain = self.dns_cache.get(target)
                if domain:
                    display_dest = f"{domain} ({target})"

            self.view_traffic.add_record(category, display_dest)

        if "FATAL" in text or "The object already exists" in text:
            self.retry_count += 1
            self.checker_id = 0

            if self.retry_count <= 3:
                self.view_logs.log(f"[!] Сбой интерфейса! Попытка {self.retry_count}/3 через 3 сек...")
                if self.vpn_thread:
                    self.vpn_thread.stop()
                    self.vpn_thread = None

                self.view_home.wheel.set_state("loading")
                self.view_home.lbl_status.setText(f"ПЕРЕЗАПУСК ({self.retry_count}/3)...")

                srv = None if self.current_mode == "adblock" else self.view_servers.servers[self.selected_server_idx]
                QTimer.singleShot(3000, lambda: self._start_vpn_process(srv))
            else:
                self.view_logs.log("[!] Туннель мертв. Превышен лимит попыток (3/3).")
                if self.vpn_thread:
                    self.vpn_thread.stop()
                    self.vpn_thread = None
                self.update_tray_menu(False)
                self.view_home.set_error_state()

    def _handle_disconnect(self):
        self.retry_count = 0
        self.checker_id = 0
        if self.vpn_thread:
            try: self.vpn_thread.log_signal.disconnect()
            except: pass
            self.vpn_thread.stop()
            self.vpn_thread = None
            if getattr(self, "obfuscator_thread", None):
                self.obfuscator_thread.stop()
                self.obfuscator_thread = None
            self.view_logs.log("[-] Отключено пользователем")

        self.view_stats.stop_tracking()
        self.update_tray_menu(False)
        self.view_home.set_connected_state(False)

    def _handle_process_crash(self):
        self.view_logs.log("[!] ВНИМАНИЕ: Процесс ядра (sing-box) был неожиданно завершен!")

        self.checker_id = 0
        if self.vpn_thread:
            self.vpn_thread.stop()
            self.vpn_thread = None

        if getattr(self, "obfuscator_thread", None):
            self.obfuscator_thread.stop()
            self.obfuscator_thread = None

        self.view_stats.stop_tracking()
        self.update_tray_menu(False)

        self.view_home.set_error_state()

    def closeEvent(self, event):
        self.checker_id = 0
        if self.vpn_thread:
            self.vpn_thread.is_running = False
            if getattr(self, "obfuscator_thread", None):
                self.obfuscator_thread.stop()
                self.obfuscator_thread = None
            self.vpn_thread.stop()

        CREATE_NO_WINDOW = 0x08000000
        subprocess.run(['taskkill', '/F', '/IM', 'sing-box.exe'],
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                       creationflags=CREATE_NO_WINDOW)


        event.accept()

    def _is_tun_ready(self):
        try:
            for iface in psutil.net_if_addrs().keys():
                if "forgefox" in iface.lower():
                    stats = psutil.net_if_stats().get(iface)

                    if stats and stats.isup:
                        return True

        except Exception as e:
            print("iface check error:", e)

        return False

    def _start_tun_checker(self):
        self.tun_checks = 0
        import random
        self.checker_id = random.random()
        current_id = self.checker_id

        def check():
            if self.checker_id != current_id:
                return

            if self.vpn_thread is None:
                return

            if self.current_mode == "proxy":
                self.view_home.set_connected_state(True)
                self.view_logs.log("[+] PROXY запущен (SOCKS5 127.0.0.1:1080)")
                self.update_tray_menu(True)
                return

            if self._is_tun_ready() and self.vpn_thread and self.vpn_thread.isRunning():
                self.view_home.set_connected_state(True)
                self.view_logs.log("[+] TUN интерфейс готов — подключено.")
                self.update_tray_menu(True)

                for iface in psutil.net_if_addrs().keys():
                    if "forgefox" in iface.lower():
                        self.view_stats.start_tracking(iface)
                        break

                if self.settings.value("obfuscator_enabled", False, type=bool):
                    sites_str = self.settings.value("obf_sites",
                                                    "wikipedia.org, amazon.com, weather.com, apple.com, microsoft.com")
                    sites = [s.strip() for s in sites_str.split(",") if s.strip()]
                    if sites:
                        self.obfuscator_thread = ObfuscatorThread(sites)
                        self.obfuscator_thread.start()
                        self.view_logs.log("[*] Генератор Белого Шума активирован.")

                return

            self.tun_checks += 1
            if self.tun_checks > 20:
                self.view_logs.log("[!] Интерфейс не поднялся (таймаут)")
                self.view_home.set_error_state()
                self.update_tray_menu(False)
                if self.vpn_thread:
                    self.vpn_thread.stop()
                    self.vpn_thread = None
                return

            QTimer.singleShot(500, check)

        check()

    def _create_start_menu_shortcut(self):
        try:
            import os
            from win32com.client import Dispatch

            app_data = os.getenv('APPDATA')
            shortcut_path = os.path.join(app_data, r"Microsoft\Windows\Start Menu\Programs\ForgeFox.lnk")

            if not os.path.exists(shortcut_path):
                target = sys.executable
                shell = Dispatch('WScript.Shell')
                shortcut = shell.CreateShortCut(shortcut_path)
                shortcut.Targetpath = target
                shortcut.WorkingDirectory = os.path.dirname(target)
                shortcut.IconLocation = target
                shortcut.save()
        except Exception as e:
            print(f"Shortcut error: {e}")