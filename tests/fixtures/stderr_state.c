/** Private write injection validates finite attempts, partial failures and chunk bounds. */
#define _POSIX_C_SOURCE 200809L
#define _DARWIN_C_SOURCE
#define _DEFAULT_SOURCE
#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
static unsigned mode,calls;
static size_t accepted,largest;

/** Replace writes only inside the implementation under test. @param fd/data/count Request. @return Simulated progress. */
static ssize_t controlled_write(int fd,const void* data,size_t count) {
    if(fd!=2 || data==NULL || count==0) abort();
    calls++; if(count>largest) largest=count;
    if(mode==0 || (mode==1 && calls<=2)){errno=EINTR; return -1;}
    if(mode==2) return 0;
    if(mode==3 && calls>1){errno=EIO; return -1;}
    size_t bytes=mode==1 ? 1 : mode==3 ? 2 : count;
    accepted+=bytes; return (ssize_t)bytes;
}
#define write controlled_write
/* glibc upgrades this feature macro after headers; restore the implementation definition. */
#undef _POSIX_C_SOURCE
#include "../../runtime/fern_stderr.c"

/** Reset injected output without touching real stderr. @param next Simulation. */
static void reset(unsigned next){mode=next; calls=0; accepted=0; largest=0;}

/** Assert exact heap branch and payload. @param value Result. @param error Expected code. */
static void result(int64_t value,int error){if(fern_result_is_ok(value)!=(error==0) || fern_result_unwrap(value)!=error) abort();}

/** Exercise bounded hostile write responses. @return Oracle status. */
int fern_main(void) {
    reset(0); result(fern_write_stderr("error"),3); if(calls!=65536 || accepted!=0) return 1;
    reset(1); result(fern_write_stderr("hello"),0); if(calls!=7 || accepted!=5) return 2;
    reset(2); result(fern_write_stderr("error"),3); if(calls!=1) return 3;
    reset(3); result(fern_write_stderr("error"),3); if(calls!=2 || accepted!=2) return 4;
    reset(4); char text[32770]; memset(text,'x',sizeof(text)-1); text[sizeof(text)-1]=0;
    result(fern_write_stderr(text),0); if(calls!=3 || largest!=16384 || accepted!=32769) return 5;
    reset(4); result(fern_write_stderr("\300\257"),1); if(calls!=0) return 6;
    puts("ok:state"); return 0;
}
