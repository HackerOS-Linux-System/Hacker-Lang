! Repozytorium bibliotek virus

[lib.std_core]
-> description => Standardowa biblioteka rdzeniowa HLA
-> owner => HackerOS Team
-> versions
--> 1.6.0 => https://repo.hackeros.org/libs/std_core-1.6.0.tar.gz
--> 1.6.3 => https://repo.hackeros.org/libs/std_core-1.6.3.tar.gz

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

