#pragma once

#include <rte_common.h>

/**
 * lcore main for reflect mode
 */
__rte_noreturn int lcore_main_reflect(void *arg);

/**
 * Application main function
 */
int main(int argc, char *argv[]);
