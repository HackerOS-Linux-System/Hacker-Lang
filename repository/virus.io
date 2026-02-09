! Repozytorium bibliotek virus

[hk-parser]
-> description => Parser dla języka programownia Hacker Lang (część konfiguracyjna)
-> owner => HackerOS Team
-> versions
--> 0.1.0 => https://github.com/Bytes-Repository/hk-parser/releases/download/v0.1.0/libhk_parser.rlib

[lib.net_utils]
-> description => Rozszerzone narzędzia sieciowe (TCP/UDP/SSL)
-> type => shared-lib
-> versions
--> 0.9.5
---> x86_64_linux => net_utils_0.9.5_linux.so
---> aarch64_linux => net_utils_0.9.5_arm.so
--> 1.0.0
---> x86_64_linux => net_utils_1.0.0_linux.so

[lib.graphics_engine]
-> description => Niskopoziomowy silnik graficzny (vulkan-based)
-> type => static-lib
-> build_target => rlib
-> versions
--> 2.1.0-beta
---> rlib => graphics_2.1.0.rlib
---> static => libgraphics_2.1.0.a

[security]
-> verify_signatures => true
-> gpg_key => 0xHACKER_OS_KEY_2026

