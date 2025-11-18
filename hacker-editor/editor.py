import os
from PySide6.QtWidgets import (
    QMainWindow, QPlainTextEdit, QFileDialog, QMessageBox, QStatusBar,
    QToolBar, QSplitter, QTextEdit, QVBoxLayout, QWidget
)
from PySide6.QtGui import QIcon, QKeySequence, QFont, QColor, QAction
from PySide6.QtCore import Qt, QSize
from highlighter import HackerHighlighter
from runner import ScriptRunner

class HackerEditor(QMainWindow):
    def __init__(self):
        super().__init__()
        self.setWindowTitle("Hacker Editor")
        self.resize(1000, 700)

        # Centralny splitter
        self.splitter = QSplitter(Qt.Vertical)
        self.setCentralWidget(self.splitter)

        # Edytor kodu
        self.editor = QPlainTextEdit()
        self.editor.setFont(QFont("Monospace", 11))
        self.highlighter = HackerHighlighter(self.editor.document())
        self.splitter.addWidget(self.editor)

        # Panel konsoli
        console_widget = QWidget()
        console_layout = QVBoxLayout(console_widget)
        console_layout.setContentsMargins(0, 0, 0, 0)
        self.console = QTextEdit()
        self.console.setReadOnly(True)
        self.console.setFont(QFont("Monospace", 10))
        self.console.setStyleSheet("background-color: #F8F9FA; color: #212529;")
        console_layout.addWidget(self.console)
        self.splitter.addWidget(console_widget)
        self.splitter.setSizes([500, 200])  # Domyślne proporcje

        # Status bar
        self.status_bar = QStatusBar()
        self.setStatusBar(self.status_bar)

        # Toolbar
        self.create_toolbar()

        # Menu
        self.create_menu()

        # Skróty
        self.editor.document().modificationChanged.connect(self.setWindowModified)
        self.current_file = None

        # Runner
        self.runner = ScriptRunner(self)

    def create_toolbar(self):
        toolbar = QToolBar("Main Toolbar")
        self.addToolBar(toolbar)

        # Dodanie ikon z tematu Qt dla lepszej widoczności przycisków
        new_action = QAction(QIcon.fromTheme("document-new"), "New", self)
        new_action.triggered.connect(self.new_file)
        new_action.setShortcut(QKeySequence.New)
        toolbar.addAction(new_action)

        open_action = QAction(QIcon.fromTheme("document-open"), "Open", self)
        open_action.triggered.connect(self.open_file)
        open_action.setShortcut(QKeySequence.Open)
        toolbar.addAction(open_action)

        save_action = QAction(QIcon.fromTheme("document-save"), "Save", self)
        save_action.triggered.connect(self.save_file)
        save_action.setShortcut(QKeySequence.Save)
        toolbar.addAction(save_action)

        toolbar.addSeparator()

        run_action = QAction(QIcon.fromTheme("media-playback-start"), "Run", self)
        run_action.triggered.connect(self.run_script)
        run_action.setShortcut(QKeySequence("Ctrl+R"))
        toolbar.addAction(run_action)

        # Styl toolbaru - zwiększony kontrast dla lepszej widoczności
        toolbar.setStyleSheet("""
            QToolBar {
                background: #DEE2E6;
                border: 1px solid #CED4DA;
                spacing: 5px;
            }
            QToolButton {
                background: #FFFFFF;
                border: 1px solid #CED4DA;
                border-radius: 4px;
                padding: 5px;
                color: #212529;
            }
            QToolButton:hover {
                background: #E9ECEF;
            }
        """)
        toolbar.setIconSize(QSize(24, 24))  # Większe ikony dla lepszej widoczności
        toolbar.setToolButtonStyle(Qt.ToolButtonTextBesideIcon)  # Tekst obok ikony

    def create_menu(self):
        menu_bar = self.menuBar()

        file_menu = menu_bar.addMenu("File")
        file_menu.addAction("New", self.new_file, QKeySequence.New)
        file_menu.addAction("Open", self.open_file, QKeySequence.Open)
        file_menu.addAction("Save", self.save_file, QKeySequence.Save)
        file_menu.addAction("Save As", self.save_as_file, QKeySequence.SaveAs)
        file_menu.addSeparator()
        file_menu.addAction("Exit", self.close, QKeySequence.Quit)

        run_menu = menu_bar.addMenu("Run")
        run_menu.addAction("Run Script", self.run_script, QKeySequence("Ctrl+R"))
        run_menu.addAction("Clear Console", self.clear_console)

        view_menu = menu_bar.addMenu("View")
        view_menu.addAction("Toggle Console", self.toggle_console)

        help_menu = menu_bar.addMenu("Help")
        help_menu.addAction("About", self.show_about)

    def new_file(self):
        if self.maybe_save():
            self.editor.clear()
            self.current_file = None
            self.setWindowTitle("Hacker Editor - New File")
            self.status_bar.showMessage("New file created")

    def open_file(self):
        if self.maybe_save():
            file_name, _ = QFileDialog.getOpenFileName(self, "Open File", "", "Hacker Files (*.hacker);;All Files (*)")
            if file_name:
                try:
                    with open(file_name, "r") as f:
                        self.editor.setPlainText(f.read())
                    self.current_file = file_name
                    self.setWindowTitle(f"Hacker Editor - {os.path.basename(file_name)}[*]")
                    self.status_bar.showMessage(f"Opened {file_name}")
                except Exception as e:
                    QMessageBox.critical(self, "Error", f"Could not open file: {e}")

    def save_file(self):
        if self.current_file:
            self._save_to_file(self.current_file)
        else:
            self.save_as_file()

    def save_as_file(self):
        file_name, _ = QFileDialog.getSaveFileName(self, "Save File", "", "Hacker Files (*.hacker);;All Files (*)")
        if file_name:
            self._save_to_file(file_name)

    def _save_to_file(self, file_name):
        try:
            with open(file_name, "w") as f:
                f.write(self.editor.toPlainText())
            self.current_file = file_name
            self.setWindowTitle(f"Hacker Editor - {os.path.basename(file_name)}[*]")
            self.editor.document().setModified(False)
            self.status_bar.showMessage(f"Saved {file_name}")
        except Exception as e:
            QMessageBox.critical(self, "Error", f"Could not save file: {e}")

    def maybe_save(self):
        if self.editor.document().isModified():
            ret = QMessageBox.warning(self, "Unsaved Changes",
                                      "The document has been modified.\nDo you want to save your changes?",
                                      QMessageBox.Save | QMessageBox.Discard | QMessageBox.Cancel)
            if ret == QMessageBox.Save:
                self.save_file()
                return not self.editor.document().isModified()
            elif ret == QMessageBox.Cancel:
                return False
        return True

    def run_script(self):
        if not self.current_file:
            if self.maybe_save():
                self.save_as_file()
            if not self.current_file:
                return
        self.runner.run_script(self.current_file)

    def clear_console(self):
        self.console.clear()
        self.status_bar.showMessage("Console cleared")

    def toggle_console(self):
        if self.splitter.sizes()[1] == 0:
            self.splitter.setSizes([self.splitter.sizes()[0], 200])
        else:
            self.splitter.setSizes([self.splitter.sizes()[0], 0])

    def show_about(self):
        QMessageBox.about(self, "About Hacker Editor",
                          "Hacker Editor v0.0.7 - Advanced IDLE for .hacker language.\n"
                          "Built with PySide6 and Python.\n"
                          "Features: Syntax highlighting, console output, toolbar, and more.")

    def closeEvent(self, event):
        if self.maybe_save():
            event.accept()
        else:
            event.ignore()
