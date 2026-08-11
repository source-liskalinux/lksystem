#define LKSYSTEM "/sbin/lksystem"
#define STOPIT "/etc/lksystem/stopit"
#define REBOOT "/etc/lksystem/reboot"
#define NOSYNC "/etc/lksystem/nosync"
#define CTRLALTDEL "/etc/lksystem/ctrlaltdel"

#ifdef lksys_USE_SYSLIMITS
#include <limits.h>
#ifndef PATH_MAX
#define PATH_MAX 256
#endif
#define BUFSIZE (PATH_MAX > 256 ? PATH_MAX : 256)
#else
#define BUFSIZE 256
#endif
