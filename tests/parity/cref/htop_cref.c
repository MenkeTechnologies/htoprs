/* C reference harness for the htoprs parity suite.
 *
 * Compiled against htop's REAL XUtils.c (version 3.5.1) so the parity tests
 * diff the Rust port's output against the genuine C implementation, not a
 * reimplementation. Dispatch: argv[1] = function name, remaining argv = inputs;
 * result is printed to stdout in a canonical format the Rust side reproduces.
 *
 * Build (see tests/parity/xutils_parity.rs):
 *   cc -std=c11 -I<this-dir> -I<htop-src> -DHEADER_CRT \
 *      -o htop_cref htop_cref.c <htop-src>/XUtils.c
 */
#include "config.h"
#include "XUtils.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

/* Stubs for the two CRT symbols XUtils.c references (CRT.h is compiled out). */
void CRT_done(void) {}
void CRT_fatalError(const char* note) {
    fprintf(stderr, "fatal: %s\n", note);
    exit(1);
}

static void print_array(char** arr, size_t n) {
    printf("n=%zu\n", n);
    for (size_t i = 0; i < n; i++)
        printf("[%s]\n", arr[i]);
}

int main(int argc, char** argv) {
    if (argc < 2)
        return 2;
    const char* fn = argv[1];

    if (!strcmp(fn, "countDigits")) {
        printf("%zu\n", countDigits(strtoul(argv[2], 0, 10), strtoul(argv[3], 0, 10)));
    } else if (!strcmp(fn, "countTrailingZeros")) {
        printf("%u\n", countTrailingZeros((unsigned)strtoul(argv[2], 0, 10)));
    } else if (!strcmp(fn, "compareRealNumbers")) {
        printf("%d\n", compareRealNumbers(strtod(argv[2], 0), strtod(argv[3], 0)));
    } else if (!strcmp(fn, "sumPositiveValues")) {
        /* argv[2] = comma-separated doubles (or "" for empty). %.17g round-trips
         * the exact IEEE-754 double so the Rust side can parse and bit-compare. */
        double buf[256];
        size_t n = 0;
        char* s = strdup(argv[2] ? argv[2] : "");
        for (char* tok = strtok(s, ","); tok && n < 256; tok = strtok(0, ","))
            buf[n++] = strtod(tok, 0);
        printf("%.17g\n", sumPositiveValues(buf, n));
        free(s);
    } else if (!strcmp(fn, "String_cat")) {
        char* r = String_cat(argv[2], argv[3]);
        printf("[%s]\n", r);
        free(r);
    } else if (!strcmp(fn, "String_trim")) {
        char* r = String_trim(argv[2]);
        printf("[%s]\n", r);
        free(r);
    } else if (!strcmp(fn, "String_contains_i")) {
        printf("%d\n", String_contains_i(argv[2], argv[3], atoi(argv[4])) ? 1 : 0);
    } else if (!strcmp(fn, "String_split")) {
        size_t n = 0;
        char** a = String_split(argv[2], argv[3][0], &n);
        print_array(a, n);
        String_freeArray(a);
    } else if (!strcmp(fn, "String_splitFirst")) {
        size_t n = 0;
        char** a = String_splitFirst(argv[2], argv[3][0], &n);
        print_array(a, n);
        String_freeArray(a);
    } else {
        fprintf(stderr, "unknown fn: %s\n", fn);
        return 3;
    }
    return 0;
}
