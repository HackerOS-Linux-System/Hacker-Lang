import sys
from PySide6.QtWidgets import QApplication
from editor import HackerEditor

if __name__ == "__main__":
    app = QApplication(sys.argv)
    app.setStyle("Fusion")  # Ładniejszy styl
    editor = HackerEditor()
    editor.show()
    sys.exit(app.exec())
