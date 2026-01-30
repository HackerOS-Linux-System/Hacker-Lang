#include <rte_cycles.h>
#include <rte_lcore.h>
#include <rte_mbuf.h>
#include <rte_ether.h>
#include <rte_ip.h>
#include <rte_byteorder.h>
#include <signal.h>
#include <unistd.h>
#include <stdint.h>
#include <stdio.h>
#include <inttypes.h>
#include <rte_eal.h>
#include <rte_ethdev.h>
#include <rte_mempool.h>
#include <string.h>
#include <stdlib.h>

#define RX_RING_SIZE 1024
#define TX_RING_SIZE 1024
#define NUM_MBUFS 8191
#define MBUF_CACHE_SIZE 250
#define BURST_SIZE 32

static volatile int force_quit = 0;
static uint16_t nb_rx_queues;

static const struct rte_eth_conf port_conf_default = {
    .rxmode = {
        .offloads = 0,
    },
    .txmode = {
        .offloads = 0,
    },
};

static void signal_handler(int signum) {
    force_quit = 1;
}

static void print_port_stats(void) {
    struct rte_eth_stats stats;
    uint16_t port;
    printf("\nPort statistics ====================================\n");
    RTE_ETH_FOREACH_DEV(port) {
        rte_eth_stats_get(port, &stats);
        printf("Port %u: RX packets: %"PRIu64" TX packets: %"PRIu64" Dropped: %"PRIu64"\n",
               port, stats.ipackets, stats.opackets, stats.imissed + stats.ierrors + stats.rx_nombuf);
    }
    printf("====================================================\n");
}

/* Initializes a given port using global settings and with the RX buffers coming from the mbuf_pool passed as a parameter. */
static inline int
port_init(uint16_t port, struct rte_mempool *mbuf_pool)
{
    struct rte_eth_conf port_conf = port_conf_default;
    uint16_t rx_rings = nb_rx_queues;
    uint16_t tx_rings = nb_rx_queues;
    uint16_t nb_rxd = RX_RING_SIZE;
    uint16_t nb_txd = TX_RING_SIZE;
    int retval;
    uint16_t q;
    struct rte_eth_dev_info dev_info;
    struct rte_eth_txconf txconf;

    if (!rte_eth_dev_is_valid_port(port))
        return -1;

    retval = rte_eth_dev_info_get(port, &dev_info);
    if (retval != 0)
        return retval;

    if (dev_info.tx_offload_capa & RTE_ETH_TX_OFFLOAD_MBUF_FAST_FREE)
        port_conf.txmode.offloads |= RTE_ETH_TX_OFFLOAD_MBUF_FAST_FREE;

    if (rx_rings > 1) {
        port_conf.rxmode.mq_mode = RTE_ETH_MQ_RX_RSS;
        port_conf.rx_adv_conf.rss_conf.rss_hf = RTE_ETH_RSS_IP | RTE_ETH_RSS_UDP | RTE_ETH_RSS_TCP;
        port_conf.rx_adv_conf.rss_conf.rss_key = NULL;
        port_conf.rx_adv_conf.rss_conf.rss_key_len = 0;
    }

    /* Configure the Ethernet device. */
    retval = rte_eth_dev_configure(port, rx_rings, tx_rings, &port_conf);
    if (retval != 0)
        return retval;

    retval = rte_eth_dev_adjust_nb_rx_tx_desc(port, &nb_rxd, &nb_txd);
    if (retval != 0)
        return retval;

    /* Allocate and set up RX queues per Ethernet port. */
    for (q = 0; q < rx_rings; q++) {
        retval = rte_eth_rx_queue_setup(port, q, nb_rxd,
                                        rte_eth_dev_socket_id(port), NULL, mbuf_pool);
        if (retval < 0)
            return retval;
    }

    txconf = dev_info.default_txconf;
    txconf.offloads = port_conf.txmode.offloads;
    /* Allocate and set up TX queues per Ethernet port. */
    for (q = 0; q < tx_rings; q++) {
        retval = rte_eth_tx_queue_setup(port, q, nb_txd,
                                        rte_eth_dev_socket_id(port), &txconf);
        if (retval < 0)
            return retval;
    }

    /* Start the Ethernet port. */
    retval = rte_eth_dev_start(port);
    if (retval < 0)
        return retval;

    /* Display the port MAC address. */
    struct rte_ether_addr addr;
    rte_eth_macaddr_get(port, &addr);
    printf("Port %u MAC: %02" PRIx8 " %02" PRIx8 " %02" PRIx8
    " %02" PRIx8 " %02" PRIx8 " %02" PRIx8 "\n",
    port,
    addr.addr_bytes[0], addr.addr_bytes[1],
    addr.addr_bytes[2], addr.addr_bytes[3],
    addr.addr_bytes[4], addr.addr_bytes[5]);

    /* Enable RX in promiscuous mode for the Ethernet device. */
    rte_eth_promiscuous_enable(port);

    return 0;
}

/* The lcore main. This is the main thread that does the work, reading from an input port and writing to an output port. */
static int
lcore_main(void *arg)
{
    uint16_t queue_id = (uintptr_t)arg;
    uint16_t port;
    static uint64_t total_cycles = 0;
    static uint64_t total_pkts = 0;
    /*
     * Check that the port is on the same NUMA node as the polling thread
     * for best performance.
     */
    RTE_ETH_FOREACH_DEV(port)
    if (rte_eth_dev_socket_id(port) > 0 &&
        rte_eth_dev_socket_id(port) != (int)rte_socket_id())
        printf("WARNING, port %u is on remote NUMA node to "
        "polling thread.\n\tPerformance will "
        "not be optimal.\n", port);

    printf("\nCore %u (queue %u) forwarding packets.\n",
           rte_lcore_id(), queue_id);

    /* Run until the application is quit or killed. */
    while (!force_quit) {
        RTE_ETH_FOREACH_DEV(port) {
            struct rte_mbuf *bufs[BURST_SIZE];
            uint64_t rx_cycles = rte_get_timer_cycles();
            const uint16_t nb_rx = rte_eth_rx_burst(port, queue_id, bufs, BURST_SIZE);
            if (unlikely(nb_rx == 0))
                continue;

            uint16_t nb_tx_prep = 0;
            for (uint16_t buf = 0; buf < nb_rx; buf++) {
                struct rte_mbuf *m = bufs[buf];
                struct rte_ether_hdr *eth_hdr = rte_pktmbuf_mtod(m, struct rte_ether_hdr *);
                if (eth_hdr->ether_type != rte_cpu_to_be_16(RTE_ETHER_TYPE_IPV4)) {
                    bufs[nb_tx_prep++] = m;
                    continue;
                }
                struct rte_ipv4_hdr *ip_hdr = (struct rte_ipv4_hdr *)(eth_hdr + 1);
                uint32_t forbidden_ip = rte_be_to_cpu_32(RTE_IPV4(192, 168, 1, 0));
                if (ip_hdr->src_addr == forbidden_ip) {
                    rte_pktmbuf_free(m);
                    continue;
                }
                // Example modification: Change destination IP (NAT-like)
                // ip_hdr->dst_addr = rte_cpu_to_be_32(RTE_IPV4(10, 0, 0, 1));
                // ip_hdr->hdr_checksum = 0;
                // ip_hdr->hdr_checksum = rte_ipv4_cksum(ip_hdr);
                // Example: Add VLAN tag (requires sufficient headroom in mbuf)
                // rte_vlan_insert(m);
                bufs[nb_tx_prep++] = m;
            }

            uint64_t tx_cycles = rte_get_timer_cycles();
            total_cycles += (tx_cycles - rx_cycles);
            total_pkts += nb_tx_prep;

            const uint16_t nb_tx = rte_eth_tx_burst(port ^ 1, queue_id, bufs, nb_tx_prep);
            if (unlikely(nb_tx < nb_tx_prep)) {
                for (uint16_t buf = nb_tx; buf < nb_tx_prep; buf++)
                    rte_pktmbuf_free(bufs[buf]);
            }
        }
    }

    // Print per-core latency stats on exit
    if (total_pkts > 0) {
        double avg_latency_us = (double)total_cycles / total_pkts / rte_get_timer_hz() * 1000000.0;
        printf("Core %u: Average latency: %.2f us, Total packets: %"PRIu64"\n",
               rte_lcore_id(), avg_latency_us, total_pkts);
    }

    return 0;
}

/* The main function, which does initialization and calls the per-lcore functions. */
int
main(int argc, char *argv[])
{
    struct rte_mempool *mbuf_pool;
    unsigned nb_ports;
    uint16_t portid;
    unsigned nb_lcores = rte_lcore_count();

    // Prepare EAL arguments to include --no-huge if not present
    bool has_no_huge = false;
    for (int i = 1; i < argc; i++) {
        if (strcmp(argv[i], "--no-huge") == 0) {
            has_no_huge = true;
            break;
        }
    }

    int eal_argc = has_no_huge ? argc : argc + 1;
    char **eal_argv = (char **)malloc(sizeof(char *) * (eal_argc + 1));
    if (eal_argv == NULL) {
        fprintf(stderr, "Cannot allocate memory for eal_argv\n");
        return -1;
    }

    for (int i = 0; i < argc; i++) {
        eal_argv[i] = argv[i];
    }

    if (!has_no_huge) {
        eal_argv[argc] = "--no-huge";
    }

    eal_argv[eal_argc] = NULL;

    /* Initialize the Environment Abstraction Layer (EAL). */
    int ret = rte_eal_init(eal_argc, eal_argv);
    free(eal_argv);
    if (ret < 0)
        rte_exit(EXIT_FAILURE, "Error with EAL initialization\n");

    argc -= ret;
    argv += ret;

    /* Set up signals */
    signal(SIGINT, signal_handler);
    signal(SIGTERM, signal_handler);

    /* Check that there is an even number of ports to send/receive on. */
    nb_ports = rte_eth_dev_count_avail();
    if (nb_ports < 2 || (nb_ports & 1))
        rte_exit(EXIT_FAILURE, "Error: number of ports must be even\n");

    nb_rx_queues = (nb_lcores > 1) ? nb_lcores - 1 : 1;

    /* Creates a new mempool in memory to hold the mbufs. */
    mbuf_pool = rte_pktmbuf_pool_create("MBUF_POOL", NUM_MBUFS * nb_ports,
                                        MBUF_CACHE_SIZE, 0, RTE_MBUF_DEFAULT_BUF_SIZE,
                                        rte_socket_id());

    if (mbuf_pool == NULL)
        rte_exit(EXIT_FAILURE, "Cannot create mbuf pool\n");

    /* Initialize all ports. */
    RTE_ETH_FOREACH_DEV(portid)
    if (port_init(portid, mbuf_pool) != 0)
        rte_exit(EXIT_FAILURE, "Cannot init port %"PRIu16 "\n",
                 portid);

        if (nb_lcores == 1) {
            printf("\nRunning in single-lcore mode.\n");
            lcore_main((void *)0);
        } else {
            int queue = 0;
            unsigned lcore_id;
            RTE_LCORE_FOREACH_WORKER(lcore_id) {
                rte_eal_remote_launch(lcore_main, (void *)(uintptr_t)queue, lcore_id);
                queue++;
            }

            /* Master core handles stats and shutdown */
            while (!force_quit) {
                sleep(10);
                print_port_stats();
            }

            /* Wait for workers to exit */
            RTE_LCORE_FOREACH_WORKER(lcore_id) {
                rte_eal_wait_lcore(lcore_id);
            }
        }

        /* Cleanup */
        print_port_stats(); // Final stats
        RTE_ETH_FOREACH_DEV(portid) {
            rte_eth_dev_stop(portid);
            rte_eth_dev_close(portid);
        }
        rte_mempool_free(mbuf_pool);

        return 0;
}
