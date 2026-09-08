import sys
import ctypes
from PyQt6.QtWidgets import QApplication
from PyQt6.QtGui import QIcon

from package.utils import resource_path
from package.ui import MainWindow

if __name__ == "__main__":
    myappid = u'ForgeFox.VPN.3.0.4'
    ctypes.windll.shell32.SetCurrentProcessExplicitAppUserModelID(myappid)


    def is_admin():
        try:
            return ctypes.windll.shell32.IsUserAnAdmin()
        except:
            return False


    if not is_admin():
        params = " ".join([f'"{arg}"' for arg in sys.argv[1:]])
        ctypes.windll.shell32.ShellExecuteW(None, "runas", sys.executable, params, None, 1)
        sys.exit()

    app = QApplication(sys.argv)
    app.setWindowIcon(QIcon(resource_path("static/icon.ico")))
    app.setOrganizationName("ForgeFox")
    app.setApplicationName("ForgeFoxVPN")

    window = MainWindow()
    window.show()
    sys.exit(app.exec())