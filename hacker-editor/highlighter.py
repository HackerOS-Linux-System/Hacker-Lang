from PySide6.QtGui import QSyntaxHighlighter, QTextCharFormat, QColor, QFont
from PySide6.QtCore import QRegularExpression

class HackerHighlighter(QSyntaxHighlighter):
    def __init__(self, parent=None):
        super().__init__(parent)
        self.highlighting_rules = []

        # Ulepszone formaty z lepszymi kolorami
        keyword_format = QTextCharFormat()
        keyword_format.setForeground(QColor("#007BFF"))  # Niebieski
        keyword_format.setFontWeight(QFont.Bold)

        variable_format = QTextCharFormat()
        variable_format.setForeground(QColor("#6F42C1"))  # Fioletowy

        command_format = QTextCharFormat()
        command_format.setForeground(QColor("#28A745"))  # Zielony

        comment_format = QTextCharFormat()
        comment_format.setForeground(QColor("#DC3545"))  # Czerwony
        comment_format.setFontItalic(True)

        config_format = QTextCharFormat()
        config_format.setForeground(QColor("#17A2B8"))  # Cyjan

        loop_format = QTextCharFormat()
        loop_format.setForeground(QColor("#FD7E14"))  # Pomarańczowy

        conditional_format = QTextCharFormat()
        conditional_format.setForeground(QColor("#6F42C1"))  # Fioletowy

        background_format = QTextCharFormat()
        background_format.setForeground(QColor("#17A2B8"))  # Cyjan

        # Reguły dla prefiksów
        prefixes = [
            ("//", keyword_format),    # Deps
            ("#", keyword_format),     # Libs
            ("@", variable_format),    # Vars
            (">", command_format),     # Cmds
            ("=", loop_format),        # Loops
            ("\\?", conditional_format),  # Conditionals
            ("&", background_format),  # Background
            ("!", comment_format),     # Comments
            ("\\[", config_format),    # Config start
            ("\\]", config_format),    # Config end
        ]

        for pattern, fmt in prefixes:
            rule = (QRegularExpression(f"^{pattern}"), fmt)
            self.highlighting_rules.append(rule)

        # Cała linia komentarza
        comment_rule = (QRegularExpression("!.*"), comment_format)
        self.highlighting_rules.append(comment_rule)

        # Dodatkowe reguły dla zmiennych w komendach
        variable_in_cmd = (QRegularExpression(r"\$\w+"), variable_format)
        self.highlighting_rules.append(variable_in_cmd)

    def highlightBlock(self, text):
        for pattern, fmt in self.highlighting_rules:
            it = pattern.globalMatch(text)
            while it.hasNext():
                match = it.next()
                self.setFormat(match.capturedStart(), match.capturedLength(), fmt)
