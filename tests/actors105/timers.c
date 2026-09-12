/** FernSim drives adversarial late-send order through the production native actor scheduler. */
#define _POSIX_C_SOURCE 200809L
#include <time.h>
static int simulated_clock(clockid_t,struct timespec*);
#define clock_gettime simulated_clock
#include "../../runtime/fern_managed.c"
#undef clock_gettime
#include "fernsim.h"
#include <stdio.h>
#undef assert
#define assert(condition) do { if(!(condition)) { \
    fprintf(stderr,"check failed at line %d: %s\n",__LINE__,#condition); exit(1); \
} } while(0)

#define RECEIVERS 32
static void* allocations[100000];
static size_t allocation_count;
static uint64_t milliseconds;
static int trace[RECEIVERS],trace_count,spins;
static void* spin_frame;
static FernManagedType scalar={FERN_MANAGED_SCALAR,0,NULL,NULL};
static const FernManagedType* capture_types[]={&scalar};

/** Clock values come only from the scenario; no host sleep or timing affects assertions. */
static int simulated_clock(clockid_t id,struct timespec* output) {
    (void)id;
    output->tv_sec=(time_t)(milliseconds/1000u);
    output->tv_nsec=(long)(milliseconds%1000u)*1000000L;
    return 0;
}

/** Test-owned heap is retained until a scenario completes, then freed as one fixture boundary. */
void* fern_alloc(size_t size) {
    assert(allocation_count<100000);
    void* value=calloc(1,size); assert(value!=NULL);
    allocations[allocation_count++]=value;
    return value;
}

int64_t fern_result_ok(int64_t value) {
    int64_t* result=fern_alloc(16); result[0]=0; result[1]=value;
    return (int64_t)(intptr_t)result;
}
int64_t fern_result_err(int64_t value) {
    int64_t* result=fern_alloc(16); result[0]=1; result[1]=value;
    return (int64_t)(intptr_t)result;
}

/** Capture-bearing continuations identify the actual execution order. */
static int64_t record(FernManagedExec* exec,void* environment) {
    (void)exec;
    assert(trace_count<RECEIVERS);
    assert(spins<100);
    trace[trace_count++]=(int)((intptr_t*)environment)[1];
    return FERN_MANAGED_COMPLETE;
}

/** Keep runnable work present throughout timeout discovery and dispatch. */
static int64_t spin(FernManagedExec* exec,void* environment) {
    (void)environment;
    if(++spins==100) return FERN_MANAGED_COMPLETE;
    return fern_managed_continue(exec,spin_frame);
}

/** The queued messages are deliberately unmatched, and must be retired on completion. */
static void* ignore(FernManagedExec* exec,void* environment,int64_t value) {
    (void)exec; (void)environment; (void)value;
    return NULL;
}

static FernManagedFunction record_descriptor={(void*)record,record,NULL,1,capture_types,&scalar};
static FernManagedFunction spin_descriptor={(void*)spin,spin,NULL,0,NULL,&scalar};
static FernManagedFunction ignore_descriptor={(void*)ignore,NULL,ignore,0,NULL,&scalar};
static const FernManagedFunction* functions[]={&record_descriptor,&spin_descriptor,&ignore_descriptor};

/** Create a native frame with the exact descriptor capture layout. */
static void* frame(const void* code,int capture) {
    intptr_t* words=fern_alloc(16);
    words[0]=(intptr_t)code; words[1]=capture;
    return words;
}

/** One seed permutes late arrivals; native timer order still depends only on deadlines and IDs. */
static void run_seed(uint64_t seed,bool timely) {
    milliseconds=0; trace_count=0; spins=0;
    Arena* arena=arena_create(16384); assert(arena!=NULL);
    FernSim* sim=fernsim_new(arena,seed); assert(sim!=NULL);
    int64_t fault=0; FernManagedExec* root=fern_managed_new(&fault,functions,3);
    ManagedPid* pids[RECEIVERS]; uint32_t deadlines[RECEIVERS];
    for(int i=0;i<RECEIVERS;i++) {
        deadlines[i]=1+fernsim_next_u32(sim,8);
        pids[i]=fern_managed_spawn(root,frame((void*)record,i),&scalar);
        ManagedActor* actor=managed_dequeue(root->session); assert(actor==pids[i]->actor);
        assert(fern_managed_receive(&actor->exec,frame((void*)ignore,0),
            frame((void*)record,i),deadlines[i])==FERN_MANAGED_SUSPENDED);
        assert(fernsim_schedule_actor(sim,(uint32_t)i,timely ? 0 : 10));
    }
    /* Tied sends may queue a receiver before its deadline; a blocking quantum delays polling. */
    for(int i=0;i<RECEIVERS;i++) {
        FernSimEvent event={0}; assert(fernsim_step(sim,&event));
        milliseconds=event.deliver_at_ms;
        int64_t* result=(int64_t*)(intptr_t)fern_managed_send(root,pids[event.actor_id],99,&scalar);
        assert(result[0]==0);
    }
    milliseconds=10;
    spin_frame=frame((void*)spin,0); fern_managed_spawn(root,spin_frame,&scalar);
    fern_managed_run(root);
    assert(fault==0 && trace_count==RECEIVERS && spins==100);
    for(int i=1;i<RECEIVERS;i++) {
        int left=trace[i-1],right=trace[i];
        assert(deadlines[left]<deadlines[right] ||
            (deadlines[left]==deadlines[right] && left<right));
    }
    assert(root->session->live==0 && root->session->messages==0);
    assert(root->session->first==NULL && root->session->last==NULL);
    for(int i=0;i<RECEIVERS;i++) {
        assert(!pids[i]->actor->alive && pids[i]->actor->frame==NULL);
        assert(pids[i]->actor->first==NULL && pids[i]->actor->selector==NULL);
    }
    fern_managed_stop(root); arena_destroy(arena);
    for(size_t i=0;i<allocation_count;i++) free(allocations[i]);
    allocation_count=0;
}

/** A newly registered immediate timeout joins the queue after an already-due timeout. */
static int64_t start_zero(FernManagedExec* exec,void* environment) {
    (void)environment;
    return fern_managed_receive(exec,frame((void*)ignore,0),frame((void*)record,1),0);
}

/** A ready actor cannot overtake an older timer by registering an immediate timeout. */
static void staggered_timer_order(void) {
    milliseconds=0; trace_count=0; spins=0;
    FernManagedFunction zero={(void*)start_zero,start_zero,NULL,0,NULL,&scalar};
    const FernManagedFunction* table[]={&record_descriptor,&ignore_descriptor,&zero};
    int64_t fault=0; FernManagedExec* root=fern_managed_new(&fault,table,3);
    fern_managed_spawn(root,frame((void*)record,0),&scalar);
    ManagedActor* first=managed_dequeue(root->session);
    assert(fern_managed_receive(&first->exec,frame((void*)ignore,0),
        frame((void*)record,0),5)==FERN_MANAGED_SUSPENDED);
    fern_managed_spawn(root,frame((void*)start_zero,0),&scalar);
    milliseconds=5;
    fern_managed_run(root);
    assert(fault==0 && trace_count==2);
    assert(trace[0]==0 && trace[1]==1);
    fern_managed_stop(root);
    for(size_t i=0;i<allocation_count;i++) free(allocations[i]);
    allocation_count=0;
}

/** Repeated seeds verify replay as well as timely and late queued messages. */
int main(void) {
    staggered_timer_order();
    for(uint64_t seed=1;seed<=16;seed++) {
        for(int timely=0;timely<2;timely++) {
            run_seed(seed,timely!=0);
            int expected[RECEIVERS]; memcpy(expected,trace,sizeof(expected));
            run_seed(seed,timely!=0);
            assert(memcmp(expected,trace,sizeof(expected))==0);
        }
    }
    puts("managed FernSim timer ordering/fairness/retirement: ok");
    return 0;
}
