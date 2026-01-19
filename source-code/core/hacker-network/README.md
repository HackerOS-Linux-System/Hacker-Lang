# Roadmapa rozwoju projektu hacker-network (DPDK-based fast packet processor)
Stan na styczeń 2026 – realistyczna kolejność kroków

| Poziom | Nazwa etapu                          | Główne funkcjonalności                                      | Szacowany czas (1 osoba, full-time) | Poziom trudności     | Poziom czarności     | Największe pułapki / wyzwania                              |
|--------|--------------------------------------|-------------------------------------------------------------|--------------------------------------|----------------------|----------------------|------------------------------------------------------------|
| 0      | Dumb bridge / mirror                 | 1:1 kopiowanie pakietów między portami                      | 2–7 dni                             | ★☆☆☆☆                | ★☆☆☆☆                | NUMA, affinity, dropy przy małym burst size              |
| 1      | Solidny L2 bridge + podstawowe staty | Statystyki portów, liczniki dropów, latency, CPU usage     | 1–3 tygodnie                        | ★★☆☆☆                | ★★☆☆☆                | RSS konfiguracja, multi-queue, load balancing po rdzeniach |
| 2      | Podstawowy L3/L4 firewall + ACL      | Blokowanie po IP, port, protokół, kierunek                  | 3–8 tygodni                         | ★★☆☆☆                | ★★☆☆☆                | Fragmentacja IPv4, checksum offload vs software            |
| 3      | NAT / masquerading / port forwarding | Zmiana src/dst IP, portów, podstawowy connection tracking   | 2–5 miesięcy                        | ★★★☆☆                | ★★★☆☆                | TCP state tracking, asymmetric routing, scale              |
| 4      | Proste manipulacje warstwą aplikacyjną | DNS redirect/spoof, HTTP host rewrite, captive portal     | 4–10 miesięcy                       | ★★★★☆                | ★★★★☆                | Fragmentacja TCP, out-of-order, reassembly                 |
| 5      | Selective DPI + injection            | Rozpoznawanie protokołów, wstrzykiwanie payloadów           | 8–24 miesiące                       | ★★★★☆ – ★★★★★       | ★★★★☆ – ★★★★★       | QUIC, TLS 1.3, Encrypted SNI/ECH, performance              |
| 6      | Aktywny MitM (TLS interception)      | Pełne rozbijanie TLS 1.2/1.3, HSTS bypass, cert spoofing    | 1.5–4+ lata + ciągłe utrzymanie     | ★★★★★                | ★★★★★                | Certificate pinning, HPKP (legacy), CAA, Certificate Transparency |
| 7      | Stealth / anti-detection / evasion   | Ukrywanie przed EDR, NDR, honeypotami, fingerprintingiem    | Ciągłe, nigdy nie kończy się        | ★★★★★                | ★★★★★                | Timing analysis, machine learning based detection          |
| 8      | Next-gen evasion (2026–2028+)        | QUICv2, HTTP/3 full proxy, ECH, post-quantum crypto bypass  | 3+ lata od teraz                    | ★★★★★+               | ★★★★★+               | Bardzo mała liczba ludzi na świecie potrafi to robić dobrze |

---

# Ścieżka E – Router / mały software router z DPDK
Realistyczna wersja dla szybkiego routera 10/25/40/100 Gbit/s (dom/firma/small-ISP/edge)

| Etap  | Co konkretnie robimy                                      | Kluczowe funkcjonalności                                      | Trudność     | Realny czas (1 osoba) | Największe wyzwania                                      |
|-------|-----------------------------------------------------------|----------------------------------------------------------------|--------------|------------------------|----------------------------------------------------------|
| E-0   | Podstawowy L3 forwarding                                  | Routing statyczny, ARP, ICMP                                   | ★★☆☆☆        | 2–6 tygodni            | Prawidłowe checksumy, TTL decrement, fragmentation       |
| E-1   | Pełny IPv4 routing + podstawowy Linux-like interfejs      | Statyczne trasy, default gateway, multiple subnets            | ★★★☆☆        | 1.5–4 miesiące         | Route table lookup (lpm), multi-port routing             |
| E-2   | Dynamiczny routing (BGP/OSPF)                             | FRR + DPDK datapath, albo własny mini-BGP/OSPF                 | ★★★★☆        | 4–12 miesięcy          | Control plane vs data plane separation, stability        |
| E-3   | NAT44 + masquerading + connection tracking                | SNAT, DNAT, full cone / symmetric NAT, hair-pinning            | ★★★★☆        | 3–8 miesięcy           | Bardzo wysoki PPS, asymetryczny routing, scale           |
| E-4   | QoS / traffic shaping / priority queuing                  | HTB-like, FQ-CoDel / CAKE w userspace, DSCP marking            | ★★★★☆        | 4–10 miesięcy          | Bardzo trudne przy 40/100G, timing, burst control        |
| E-5   | Firewall z stateful inspection                            | Conntrack + zone-based policy, podobny do nftables/iptables   | ★★★★☆–★★★★★ | 6–15 miesięcy          | Bardzo dużo pamięci, wydajność przy tysiącach połączeń   |
| E-6   | IPv6 + NAT66 / NPTv6 / prefix delegation                  | Pełny dual-stack, RA, DHCPv6-PD, 6to4/4in6/6in4 tunnels        | ★★★★☆        | +4–12 miesięcy         | Bardzo mała liczba dobrych implementacji w userspace     |
| E-7   | WireGuard / IPsec w datapath                              | Szybkie tunele bez kernel crypto                               | ★★★★★        | +6–18 miesięcy         | Crypto offload vs software, key management               |
| E-8   | SD-WAN light / multi-WAN load-balance/failover            | Policy-based routing, link monitoring, dynamic path selection | ★★★★★        | +8–24 miesiące         | Detekcja awarii <1s, smooth failover                     |
| E-9   | Next-gen features (2026–2028+)                            | QUIC proxy, HTTP/3 aware routing, ECH handling, DPI QoS       | ★★★★★+       | 2+ lata                | Prawie nikt tego nie robi dobrze w userspace             |
