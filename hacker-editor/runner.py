import subprocess
from PySide6.QtCore import QThread, Signal
from PySide6.QtWidgets import QMessageBox

class RunnerThread(QThread):
    output_signal = Signal(str)
    finished_signal = Signal(int, str)

    def __init__(self, command):
        super().__init__()
        self.command = command

    def run(self):
        try:
            process = subprocess.Popen(self.command, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, bufsize=1)
            stdout, stderr = process.communicate()
            output = stdout + stderr
            self.output_signal.emit(output)
            self.finished_signal.emit(process.returncode, output)
        except Exception as e:
            self.finished_signal.emit(1, str(e))

class ScriptRunner:
    def __init__(self, editor):
        self.editor = editor
        self.thread = None

    def run_script(self, file_path):
        # Zakładamy, że hackerc jest w PATH; jeśli nie, dostosuj ścieżkę
        # Na przykład: command = ["/usr/bin/hackerc", "run", file_path]
        # Ale według kodu użytkownika: /usr/bin/hackerc
        command = ["/usr/bin/hackerc", "run", file_path]
        self.editor.console.append("Running script: " + file_path)
        self.thread = RunnerThread(command)
        self.thread.output_signal.connect(self.handle_output)
        self.thread.finished_signal.connect(self.handle_finished)
        self.thread.start()

    def handle_output(self, output):
        self.editor.console.append(output)

    def handle_finished(self, returncode, output):
        if returncode == 0:
            self.editor.console.append("Execution completed successfully!")
            self.editor.status_bar.showMessage("Run successful")
        else:
            self.editor.console.append("Execution failed.")
            self.editor.status_bar.showMessage("Run failed")
            if output:
                QMessageBox.critical(self.editor, "Run Error", output)
