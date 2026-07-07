use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ExternRuntime {
    Shell,   // [shell]  — bash/sh
    Python,  // [python] — python3
    Java,    // [java]   — java -jar lub javac+java
    Elf,     // [elf]    — bezpośrednia binarka
    So,      // [so]     — biblioteka .so (dlopen)
}

impl ExternRuntime {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.trim().to_lowercase().as_str() {
            "shell" | "sh" | "bash"   => Some(ExternRuntime::Shell),
            "python" | "python3" | "py" => Some(ExternRuntime::Python),
            "java" | "jar" | "jvm"    => Some(ExternRuntime::Java),
            "elf" | "bin" | "binary"  => Some(ExternRuntime::Elf),
            "so" | "dylib" | "native" => Some(ExternRuntime::So),
            _                          => None,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            ExternRuntime::Shell  => "shell",
            ExternRuntime::Python => "python",
            ExternRuntime::Java   => "java",
            ExternRuntime::Elf    => "elf",
            ExternRuntime::So     => "so",
        }
    }

    /// Komenda uruchamiająca runtime
    pub fn launcher(&self) -> &'static str {
        match self {
            ExternRuntime::Shell  => "bash",
            ExternRuntime::Python => "python3",
            ExternRuntime::Java   => "java",
            ExternRuntime::Elf    => "",    // bezpośrednie exec
            ExternRuntime::So     => "",    // dlopen
        }
    }
}

impl std::fmt::Display for ExternRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}]", self.name())
    }
}
