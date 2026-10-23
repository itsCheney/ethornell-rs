#ifndef ETHORNELL_MIKAGE_H
#define ETHORNELL_MIKAGE_H
#include <stdint.h>
/* Calls the Mikage player adds next to the host ABI (see MIKAGE.md). */
#ifdef __cplusplus
extern "C" {
#endif
/* level: 1 error, 2 warn, 3 info, 4 debug, 5 trace. Strings are UTF-8, NUL
 * terminated and valid only during the call, which can come from any thread. */
typedef void (*EthornellMikageLogCallback)(int32_t level, const char *target, const char *message);
/* Forwards engine log events and panic messages to callback (NULL stops).
 * filter is a tracing EnvFilter directive; NULL keeps "ethornell=warn,wgpu=warn".
 * Only the first call's filter takes effect. */
void ethornell_mikage_set_log_callback(EthornellMikageLogCallback callback, const char *filter);
/* The game's own screen size in its pixels (1280x720 until the game sets one).
 * Returns 0, or a negative ABI status. */
int32_t ethornell_mikage_screen_size(uint64_t handle, uint32_t *width, uint32_t *height);
/* The launcher executable for the next ethornell_ios_create on this thread: a
 * file name in the resource root such as "BGI-CHS.exe". The files a patch
 * packed into it with Enigma Virtual Box win over the game's archives, as
 * they do on Windows. NULL or "" clears it; create uses it once. Returns 0, or
 * a negative ABI status. */
int32_t ethornell_mikage_set_launcher(const char *name);
/* A font file (and TTC face) for the next ethornell_ios_create on this thread
 * that draws the characters the config's font lacks, scaled to its em: with a
 * game's own font as config font_path, the host's font behind it. NULL or ""
 * clears it; create uses it once. Returns 0, or a negative ABI status. */
int32_t ethornell_mikage_set_fallback_font(const char *path, uint32_t face);
#ifdef __cplusplus
}
#endif
#endif
