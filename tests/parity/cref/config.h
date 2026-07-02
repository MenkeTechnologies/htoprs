/* Minimal config.h so htop's XUtils.c compiles standalone for the parity
 * reference harness. htop's source tree ships no config.h (it is autotools-
 * generated), so this one is found first via `-I` on this directory. We only
 * need the few macros XUtils.c / XUtils.h reference; the heavy CRT.h is
 * neutralized with `-DHEADER_CRT` and the two CRT symbols it calls are stubbed
 * in htop_cref.c. */
#ifndef HTOPRS_CREF_CONFIG_H
#define HTOPRS_CREF_CONFIG_H

#define PACKAGE "htop"
#define VERSION "3.5.1"
#define HAVE_STRNLEN 1

/* Intentionally NOT defining HAVE_BUILTIN_CTZ: the Rust port implements htop's
 * `!HAVE_BUILTIN_CTZ` fallback (the mod-37 table), so the C reference must
 * compile that same branch for an apples-to-apples comparison — otherwise the
 * builtin path diverges at x==0 (where __builtin_ctz is undefined). */

/* CRT.h is compiled out via -DHEADER_CRT; declare the two symbols XUtils.c
 * uses so the translation unit still type-checks. */
void CRT_done(void);
void CRT_fatalError(const char* note);

#endif
