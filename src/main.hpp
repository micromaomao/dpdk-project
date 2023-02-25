#pragma once

#include <rte_common.h>

/**
 * lcore main for reflect mode
 */
__rte_noreturn int lcore_main_reflect(void *arg);

/**
 * lcore tx main for sendrecv mode
 */
__rte_noreturn int lcore_main_send(void *arg);

/**
 * lcore rx main for sendrecv mode
 */
__rte_noreturn int lcore_main_recv(void *arg);

/**
 * Application main function
 */
int main(int argc, char *argv[]);
