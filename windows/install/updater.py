import sys
import os
import urllib.request
import time
import subprocess
from PyQt6.QtWidgets import (QApplication, QWidget, QVBoxLayout,
                             QLabel, QProgressBar, QFrame, QHBoxLayout, QGraphicsDropShadowEffect)
from PyQt6.QtCore import Qt, QThread, pyqtSignal
from PyQt6.QtGui import QColor
import ctypes

UPDATE_URL = "http://forgefoxvpn.data.forgefox.ru"
APP_EXE_NAME = "ForgeFoxVPN.exe"
SING_BOX_EXE_NAME = "sing-box.exe"

CREATE_NO_WINDOW = 0x08000000


class DownloadThread(QThread):
    progress = pyqtSignal(int)
    status = pyqtSignal(str)
    finished = pyqtSignal(bool, str)

    def run(self):
        appdata = os.getenv('APPDATA')
        install_dir = os.path.join(appdata, "ForgeFoxVPN", "bin")
        exe_path = os.path.join(install_dir, APP_EXE_NAME)
        temp_exe_path = os.path.join(install_dir, APP_EXE_NAME + ".tmp")
        version_file = os.path.join(install_dir, "last_modified.txt")
        os.makedirs(install_dir, exist_ok=True)

        try:
            self.status.emit("Проверка обновлений...")

            req_head = urllib.request.Request(UPDATE_URL, method='HEAD', headers={'User-Agent': 'ForgeFox-Updater'})
            try:
                with urllib.request.urlopen(req_head, timeout=5) as response:
                    server_last_modified = response.headers.get('Last-Modified')
            except Exception as e:
                if os.path.exists(exe_path):
                    self.status.emit("Сервер недоступен. Запуск кэша...")
                    self.finished.emit(True, exe_path)
                else:
                    self.finished.emit(False, f"Нет сети или сервера ({str(e)})")
                return

            local_last_modified = ""
            if os.path.exists(version_file):
                with open(version_file, "r") as f:
                    local_last_modified = f.read().strip()

            if server_last_modified and server_last_modified == local_last_modified and os.path.exists(exe_path):
                self.status.emit("Установлена актуальная версия. Запуск...")
                self.progress.emit(100)
                self.finished.emit(True, exe_path)
                return

            self.status.emit("Скачивание обновления...")
            req_get = urllib.request.Request(UPDATE_URL, headers={'User-Agent': 'ForgeFox-Updater'})

            with urllib.request.urlopen(req_get, timeout=10) as response:
                total_length = int(response.headers.get('content-length', 0))

                with open(temp_exe_path, 'wb') as f:
                    downloaded = 0
                    while True:
                        chunk = response.read(8192)
                        if not chunk: break
                        f.write(chunk)
                        downloaded += len(chunk)
                        if total_length:
                            calc = int(100 * downloaded / total_length)
                            self.progress.emit(calc)

            self.status.emit("Остановка процессов...")

            subprocess.run(['taskkill', '/F', '/IM', APP_EXE_NAME],
                           stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                           creationflags=CREATE_NO_WINDOW)

            subprocess.run(['taskkill', '/F', '/IM', SING_BOX_EXE_NAME],
                           stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                           creationflags=CREATE_NO_WINDOW)

            self.status.emit("Установка файла...")

            if os.path.getsize(temp_exe_path) < 1024:
                os.remove(temp_exe_path)
                raise Exception("Сервер отдал поврежденный файл (возможно, 404).")

            max_retries = 10
            replaced = False
            last_error = ""

            for i in range(max_retries):
                try:
                    if os.path.exists(exe_path):
                        os.remove(exe_path)
                    os.replace(temp_exe_path, exe_path)
                    replaced = True
                    break
                except PermissionError as e:
                    last_error = str(e)
                    self.status.emit(f"Ожидание разблокировки файла... ({i + 1}/{max_retries})")
                    time.sleep(0.5)

            if not replaced:
                raise Exception(
                    f"Не удалось получить доступ к файлу (заблокирован системой или антивирусом). Ошибка: {last_error}")

            if server_last_modified:
                with open(version_file, "w") as f:
                    f.write(server_last_modified)

            self.status.emit("Готово! Запуск...")
            self.finished.emit(True, exe_path)

        except Exception as e:
            self.finished.emit(False, str(e))
            if 'temp_exe_path' in locals() and os.path.exists(temp_exe_path):
                os.remove(temp_exe_path)


class UpdaterWindow(QWidget):
    def __init__(self):
        super().__init__()
        self.setWindowFlags(Qt.WindowType.FramelessWindowHint | Qt.WindowType.WindowStaysOnTopHint)
        self.setAttribute(Qt.WidgetAttribute.WA_TranslucentBackground)
        self.setFixedSize(400, 200)

        main_layout = QVBoxLayout(self)
        main_layout.setContentsMargins(10, 10, 10, 10)

        self.card = QFrame()
        self.card.setStyleSheet("QFrame { background-color: #09090B; border-radius: 15px; border: 1px solid #27272A; }")

        shadow = QGraphicsDropShadowEffect()
        shadow.setBlurRadius(20)
        shadow.setColor(QColor(0, 0, 0, 180))
        shadow.setOffset(0, 5)
        self.card.setGraphicsEffect(shadow)

        layout = QVBoxLayout(self.card)
        layout.setContentsMargins(30, 30, 30, 30)
        layout.setSpacing(15)

        header_layout = QHBoxLayout()
        logo = QLabel("🦊")
        logo.setStyleSheet("font-size: 28px; background: transparent; border: none;")

        title = QLabel("ForgeFox Updater")
        title.setStyleSheet("color: #FFFFFF; font-size: 18px; font-weight: 900; background: transparent; border: none;")

        header_layout.addWidget(logo)
        header_layout.addWidget(title)
        header_layout.addStretch()
        layout.addLayout(header_layout)

        self.lbl_status = QLabel("Инициализация...")
        self.lbl_status.setStyleSheet(
            "color: #A1A1AA; font-size: 12px; font-weight: bold; background: transparent; border: none;")
        layout.addWidget(self.lbl_status)

        self.progress = QProgressBar()
        self.progress.setFixedHeight(8)
        self.progress.setTextVisible(False)
        self.progress.setStyleSheet("""
            QProgressBar { background-color: #18181B; border-radius: 4px; border: none; }
            QProgressBar::chunk { background-color: #FF6B00; border-radius: 4px; }
        """)
        layout.addWidget(self.progress)
        main_layout.addWidget(self.card)

        self.thread = DownloadThread()
        self.thread.progress.connect(self.progress.setValue)
        self.thread.status.connect(self.lbl_status.setText)
        self.thread.finished.connect(self._on_finished)
        self.thread.start()

    def _on_finished(self, success, payload):
        import time
        from PyQt6.QtCore import QTimer

        if success and os.path.exists(payload):
            time.sleep(0.5)

            working_dir = os.path.dirname(payload)
            subprocess.Popen([payload], cwd=working_dir, creationflags=CREATE_NO_WINDOW)
            QApplication.quit()
        else:
            self.lbl_status.setStyleSheet(
                "color: #EF4444; font-size: 12px; font-weight: bold; background: transparent; border: none;")
            self.progress.setStyleSheet(
                "QProgressBar { background-color: #18181B; border-radius: 4px; border: none; } QProgressBar::chunk { background-color: #EF4444; border-radius: 4px; }")

            if not success:
                self.lbl_status.setText(f"Сбой: {payload}")
            else:
                self.lbl_status.setText(f"Файл не найден: {APP_EXE_NAME}")

            QTimer.singleShot(7000, QApplication.quit)


def check_and_restore_app():
    from PyQt6.QtNetwork import QLocalSocket

    socket = QLocalSocket()
    socket.connectToServer("ForgeFoxVPN_IPC")

    if socket.waitForConnected(500):
        socket.write(b"WAKE")
        socket.waitForBytesWritten(500)
        socket.disconnectFromServer()
        return True

    return False


if __name__ == "__main__":
    app = QApplication(sys.argv)

    if check_and_restore_app():
        sys.exit(0)

    window = UpdaterWindow()
    window.show()
    sys.exit(app.exec())