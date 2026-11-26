#define _POSIX_C_SOURCE 200809L
#include <errno.h>
#include <signal.h>
#include <stddef.h>
#include <stdlib.h>
#include <sys/wait.h>
#include <unistd.h>

static void reap_children(int signo)
{
    int saved_errno = errno;
    (void)signo;
    while (waitpid(-1, NULL, WNOHANG) > 0) {
    }
    errno = saved_errno;
}

static void stop_now(int signo)
{
    (void)signo;
    _exit(0);
}

static int install(int signo, void (*handler)(int), int flags)
{
    struct sigaction action;
    action.sa_handler = handler;
    action.sa_flags = flags;
    sigemptyset(&action.sa_mask);
    return sigaction(signo, &action, NULL);
}

int main(void)
{
    if (install(SIGCHLD, reap_children, SA_NOCLDSTOP | SA_RESTART) != 0
        || install(SIGTERM, stop_now, 0) != 0
        || install(SIGINT, stop_now, 0) != 0) {
        return 1;
    }
    reap_children(SIGCHLD);
    for (;;) {
        pause();
    }
}
