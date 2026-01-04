import sys
import re
import os
import subprocess

# Define some mappings for syntax translation
SYNTAX_MAP = {
    # Blocks: Replace [ with { and ] with }
    r'\[': '{',
    r'\]': '}',
    # func to function (assuming C-style)
    r'func ': 'void ',  # Simplified; in real, parse return types
    # class to struct (since C, not C++)
    r'class ': 'struct ',
    # log"message" to printf("message\n");
    r'log"(.*?)"': r'printf("\1\\n");',
    # Comments: @ to //
    r'@ (.*)': r'// \1',
    # Imports: import <std:parser> to #include <parser.h> (assuming std is system)
    r'import <(.*?):(.*?)>': r'#include <\2.h>  // From \1',
}

# Memory management flags
MANUAL_FLAG = '--- manual ---'
AUTO_FLAG = '--- automatic ---'  # or --- auto ---
DEFAULT_MEM = 'ARC'  # Automatic Reference Counting

# For ARC, we'll assume including a custom arc.h and wrapping allocations
# For manual, like Odin: use raw malloc/free with friendly wrappers

# Custom includes
ARC_INCLUDE = '#include "arc.h"  // Automatic Reference Counting\n'
MANUAL_INCLUDE = '#include "manual_mem.h"  // Manual memory like Odin\n'

# Function to transpile .hcs to .c
def transpile_hcs_to_c(input_file, output_file):
    with open(input_file, 'r') as f:
        code = f.read()

    # Detect memory management
    mem_mode = DEFAULT_MEM
    if MANUAL_FLAG in code:
        mem_mode = 'MANUAL'
        code = code.replace(MANUAL_FLAG, MANUAL_INCLUDE)
    elif AUTO_FLAG in code:
        code = code.replace(AUTO_FLAG, ARC_INCLUDE)
    else:
        # Default to ARC
        code = ARC_INCLUDE + code

    # Apply syntax mappings
    for pattern, replacement in SYNTAX_MAP.items():
        code = re.sub(pattern, replacement, code)

    # Add main if not present (simplified)
    if 'main' not in code:
        code += '\nint main() {\n    // Transpiled main\n    return 0;\n}\n'

    # For static typing: Assume declarations like var:int x = 5; -> int x = 5;
    code = re.sub(r'var:(\w+) (\w+) = (.*);', r'\1 \2 = \3;', code)

    # Write to output .c
    with open(output_file, 'w') as f:
        f.write(code)

# Function to compile .c to ELF binary using gcc
def compile_c_to_elf(c_file, output_binary):
    # Use gcc for C
    cmd = ['gcc', c_file, '-o', output_binary, '-static']  # Static binary
    subprocess.run(cmd, check=True)

# CLI for the compiler
def main():
    if len(sys.argv) < 2:
        print("Usage: python HackerScript-Compiler.py <input.hcs> [output_binary]")
        sys.exit(1)

    input_file = sys.argv[1]
    base_name = os.path.splitext(input_file)[0]
    c_file = base_name + '.c'
    output_binary = sys.argv[2] if len(sys.argv) > 2 else base_name

    # Transpile
    transpile_hcs_to_c(input_file, c_file)

    # Compile
    compile_c_to_elf(c_file, output_binary)

    print(f"Compiled {input_file} to {output_binary}")

if __name__ == "__main__":
    main()
