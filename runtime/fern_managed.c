/** Decision105A: invocation-owned cooperative actors; no native stack survives receive. */
#define _POSIX_C_SOURCE 200809L
#define _DARWIN_C_SOURCE
#include "fern_managed.h"
#include "fern_runtime.h"
#include <assert.h>
#include <stdbool.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <errno.h>
#define MANAGED_LIVE 1024u
#define MANAGED_IDS 65536u
#define MANAGED_MAILBOX 4096u
#define MANAGED_MESSAGES 65536u
#define MANAGED_BYTES (64u*1024u*1024u)
#define MANAGED_FUNCTIONS 4096u
#define MANAGED_WORK 1048576u
#define MANAGED_TIMEOUT 600000
#define FAULT_TIMEOUT 8
#define FAULT_RESOURCE 9
#define FAULT_DEADLOCK 10
#define FAULT_DESCRIPTOR 11
#define FAULT_CLOCK 12

typedef struct ManagedSession ManagedSession;
typedef struct ManagedActor ManagedActor;
typedef struct ManagedMessage ManagedMessage;
struct FernManagedExec { ManagedSession* session; ManagedActor* actor; int64_t* fault; };
struct ManagedMessage { ManagedMessage* next; int64_t value; size_t cost; uint64_t enqueued; };
struct ManagedActor {
    FernManagedExec exec;
    uint64_t id;
    bool alive, queued, waiting;
    int64_t fault;
    const FernManagedType* mailbox;
    void* frame;
    void* selector;
    void* timeout_frame;
    size_t frame_cost, selector_cost, timeout_cost, messages;
    uint64_t deadline;
    ManagedMessage* first;
    ManagedMessage* last;
    ManagedActor* next;
};
typedef struct { ManagedSession* session; ManagedActor* actor; uint64_t id; const FernManagedType* mailbox; } ManagedPid;
struct ManagedSession {
    FernManagedExec root;
    const FernManagedFunction* const* functions;
    size_t function_count, live, next_id, messages, retained;
    bool stopped;
    uint64_t next_deadline;
    ManagedActor** identities;
    ManagedActor* first;
    ManagedActor* last;
};

/** Preserve the first invocation failure. @param exec Explicit current context. @param code Stable fault. */
static void managed_fail(FernManagedExec* exec,int64_t code) {
    assert(exec!=NULL && exec->fault!=NULL);
    assert(code>=FAULT_TIMEOUT && code<=FAULT_CLOCK);
    if(*exec->fault==0) *exec->fault=code;
}

/** Charge retained logical storage before publication. @param s Owner. @param cost Bytes. @return Fits. */
static bool managed_charge(ManagedSession* s,size_t cost) {
    assert(s!=NULL);
    assert(s->retained<=MANAGED_BYTES);
    if(cost>MANAGED_BYTES-s->retained) return false;
    s->retained+=cost; return true;
}

/** Release only previously charged roots. @param s Owner. @param cost Retired logical bytes. */
static void managed_release(ManagedSession* s,size_t cost) {
    assert(s!=NULL && cost<=s->retained);
    s->retained-=cost;
}

/** Find an exact immutable compiler identity; no unknown callable fallback exists.
 * @param s Descriptor owner. @param closure Code word followed by captures. @return Known descriptor or NULL.
 */
static const FernManagedFunction* managed_function_work(ManagedSession* s,const void* closure,size_t* work) {
    assert(s!=NULL);
    if(closure==NULL) return NULL;
    if(*work>=MANAGED_WORK) return NULL;
    (*work)++;
    const void* identity=*(const void* const*)closure;
    for(size_t i=0;i<s->function_count;i++) {
        if(*work>=MANAGED_WORK) return NULL;
        (*work)++;
        if(s->functions[i]->identity==identity) return s->functions[i];
    }
    return NULL;
}

/** Resolve a callback under its own finite identity-lookup allowance.
 * @param s Immutable descriptor owner. @param closure Code/capture object. @return Exact match or NULL.
 */
static const FernManagedFunction* managed_function(ManagedSession* s,const void* closure) {
    size_t work=0; return managed_function_work(s,closure,&work);
}

/** Read monotonic milliseconds. @param result Output. @return Clock is available and representable. */
static bool managed_now(uint64_t* result) {
    assert(result!=NULL);
    struct timespec ts;
    if(clock_gettime(CLOCK_MONOTONIC,&ts)!=0 || ts.tv_sec<0 || ts.tv_nsec<0 || ts.tv_nsec>=1000000000L) return false;
    uint64_t fractional=(uint64_t)ts.tv_nsec/1000000u;
    if((uint64_t)ts.tv_sec>(UINT64_MAX-fractional)/1000u) return false;
    *result=(uint64_t)ts.tv_sec*1000u+fractional; return true;
}

/** Queue one live actor once, regardless of mailbox length. @param actor Explicit owned record. */
static void managed_enqueue(ManagedActor* actor) {
    assert(actor!=NULL && actor->exec.session!=NULL);
    if(!actor->alive || actor->queued) return;
    ManagedSession* s=actor->exec.session;
    actor->queued=true; actor->next=NULL;
    if(s->last) s->last->next=actor; else s->first=actor;
    s->last=actor;
}

/** Remove one FIFO entry while retaining its actor identity. @param s Owner. @return Record or NULL. */
static ManagedActor* managed_dequeue(ManagedSession* s) {
    assert(s!=NULL);
    ManagedActor* actor=s->first;
    if(!actor) return NULL;
    s->first=actor->next; if(!s->first) s->last=NULL;
    actor->next=NULL; actor->queued=false; return actor;
}

/** Clear every queued message root on stop or ordinary actor completion. @param actor Owner. */
static void managed_clear_messages(ManagedActor* actor) {
    assert(actor!=NULL);
    ManagedSession* s=actor->exec.session;
    for(size_t i=0;i<MANAGED_MAILBOX && actor->first;i++) {
        ManagedMessage* message=actor->first; actor->first=message->next;
        managed_release(s,message->cost); message->next=NULL; message->value=0; message->cost=0;
        assert(s->messages>0 && actor->messages>0); s->messages--; actor->messages--;
    }
    assert(actor->first==NULL && actor->messages==0);
    actor->last=NULL;
}

/** Recompute the earliest timer after its owner retires it, without reading the host clock.
 * @param s Invocation whose active receive deadlines are authoritative.
 */
static void managed_refresh_deadline(ManagedSession* s) {
    assert(s!=NULL);
    assert(s->next_id<=MANAGED_IDS);
    s->next_deadline=UINT64_MAX;
    for(size_t i=0;i<s->next_id;i++) {
        ManagedActor* actor=s->identities[i];
        if(actor->alive && actor->waiting && actor->deadline<s->next_deadline) {
            s->next_deadline=actor->deadline;
        }
    }
}

/** Retire receive closures and their deadline together. @param actor Owned record. */
static void managed_clear_receive(ManagedActor* actor) {
    assert(actor!=NULL);
    managed_release(actor->exec.session,actor->selector_cost+actor->timeout_cost);
    actor->selector=NULL; actor->timeout_frame=NULL; actor->selector_cost=0; actor->timeout_cost=0;
    uint64_t retired_deadline=actor->deadline;
    actor->deadline=UINT64_MAX; actor->waiting=false;
    ManagedSession* s=actor->exec.session;
    if(!s->stopped && retired_deadline!=UINT64_MAX && retired_deadline==s->next_deadline) {
        managed_refresh_deadline(s);
    }
}

/** Retire all strong actor roots; IDs remain permanently dead within their owning session. @param actor Record. */
static void managed_finish(ManagedActor* actor) {
    assert(actor!=NULL);
    if(!actor->alive) return;
    actor->alive=false; managed_clear_messages(actor); managed_clear_receive(actor);
    managed_release(actor->exec.session,actor->frame_cost);
    actor->frame=NULL; actor->frame_cost=0;
    assert(actor->exec.session->live>0); actor->exec.session->live--;
}

#include "fern_managed_cost.inc"

/** Validate every descriptor before publishing a context. @param fault Existing8-byte slot. @param functions Compiler table. @param count Exact length. @return Root or NULL. */
FernManagedExec* fern_managed_new(int64_t* fault,const FernManagedFunction* const* functions,int64_t count) {
    if(fault==NULL) return NULL;
    if(count<1 || count>MANAGED_FUNCTIONS || functions==NULL) { if(*fault==0) *fault=FAULT_DESCRIPTOR; return NULL; }
    size_t work=0;
    for(int64_t i=0;i<count;i++) {
        const FernManagedFunction* f=functions[i];
        if(f==NULL || f->identity==NULL || (f->step==NULL)==(f->select==NULL) || f->capture_count<0 || f->capture_count>4096 || (f->capture_count && !f->captures)) { if(*fault==0) *fault=FAULT_DESCRIPTOR; return NULL; }
        for(int64_t j=0;j<i;j++) if(++work>MANAGED_WORK || functions[j]->identity==f->identity) { if(*fault==0) *fault=FAULT_DESCRIPTOR; return NULL; }
        if(!managed_descriptor_walk(f->mailbox,true,&work)) { if(*fault==0) *fault=FAULT_DESCRIPTOR; return NULL; }
        for(int64_t j=0;j<f->capture_count;j++) if(!managed_descriptor_walk(f->captures[j],false,&work)) { if(*fault==0) *fault=FAULT_DESCRIPTOR; return NULL; }
    }
    ManagedSession* s=fern_alloc(sizeof(*s)); memset(s,0,sizeof(*s));
    s->identities=fern_alloc(MANAGED_IDS*sizeof(*s->identities)); memset(s->identities,0,MANAGED_IDS*sizeof(*s->identities));
    s->retained=sizeof(*s)+MANAGED_IDS*sizeof(*s->identities);
    s->functions=functions; s->function_count=(size_t)count;
    s->next_deadline=UINT64_MAX;
    s->root=(FernManagedExec){s,NULL,fault}; return &s->root;
}

/** Return exactly the existing8-byte current fault cell. @param exec Managed invocation. @return Borrowed cell. */
int64_t* fern_managed_fault(FernManagedExec* exec) { return exec ? exec->fault : NULL; }

/** Enqueue an entry after validating captures and mailbox identity. @param exec Caller. @param closure Entry. @param mailbox Concrete type. @return Opaque PID or NULL with fault. */
void* fern_managed_spawn(FernManagedExec* exec,void* closure,const FernManagedType* mailbox) {
    if(!exec || !exec->session || *exec->fault) return NULL;
    ManagedSession* s=exec->session; const FernManagedFunction* f=managed_function(s,closure);
    if(!f || !f->step || !managed_descriptor(mailbox,false) || (f->mailbox && f->mailbox!=mailbox)) { managed_fail(exec,FAULT_DESCRIPTOR); return NULL; }
    size_t cost=0;
    if(s->stopped || s->live>=MANAGED_LIVE || s->next_id>=MANAGED_IDS || !managed_frame_cost(s,closure,&cost) || !managed_charge(s,cost+sizeof(ManagedActor)+sizeof(ManagedPid))) { managed_fail(exec,FAULT_RESOURCE); return NULL; }
    ManagedActor* actor=fern_alloc(sizeof(*actor)); memset(actor,0,sizeof(*actor));
    actor->exec=(FernManagedExec){s,actor,&actor->fault}; actor->id=++s->next_id; actor->alive=true;
    actor->mailbox=mailbox; actor->frame=closure; actor->frame_cost=cost; actor->deadline=UINT64_MAX;
    s->identities[actor->id-1]=actor; s->live++;
    ManagedPid* pid=fern_alloc(sizeof(*pid)); *pid=(ManagedPid){s,actor,actor->id,mailbox};
    managed_enqueue(actor); return pid;
}

/** Borrow a value; failed sends neither transfer duties nor modify the mailbox.
 * @param exec Sender. @param identity Typed PID. @param value Full-width payload. @param type Concrete descriptor.
 * @return Heap Result(Unit,Int); Err3 dead/foreign, Err4 quota or rejected graph.
 */
int64_t fern_managed_send(FernManagedExec* exec,void* identity,int64_t value,const FernManagedType* type) {
    if(!exec || !exec->session || !identity) return fern_result_err(3);
    ManagedSession* s=exec->session; ManagedPid* pid=identity;
    if(pid->session!=s || pid->id==0 || pid->id>s->next_id || s->identities[pid->id-1]!=pid->actor || !pid->actor->alive || pid->mailbox!=type || s->stopped) return fern_result_err(3);
    ManagedActor* actor=pid->actor; size_t cost=0; uint64_t now=0;
    if(actor->messages>=MANAGED_MAILBOX || s->messages>=MANAGED_MESSAGES || !managed_value_cost(s,type,value,&cost) || !managed_charge(s,cost+sizeof(ManagedMessage))) return fern_result_err(4);
    ManagedMessage* message=fern_alloc(sizeof(*message)); memset(message,0,sizeof(*message));
    if(!managed_now(&now)) {
        managed_release(s,cost+sizeof(*message)); managed_fail(exec,FAULT_CLOCK);
        return fern_result_err(4);
    }
    *message=(ManagedMessage){NULL,value,cost+sizeof(*message),now};
    if(actor->last) actor->last->next=message; else actor->first=message;
    actor->last=message; actor->messages++; s->messages++;
    /* Late messages stay queued, but timer promotion determines their receiver's wake order. */
    if(actor->waiting && (actor->deadline==UINT64_MAX || now<actor->deadline)) managed_enqueue(actor);
    return fern_result_ok(0);
}

/** Install one validated continuation before releasing the old frame. @param actor Owner. @param frame New closure. @return Published. */
static bool managed_replace(ManagedActor* actor,void* frame) {
    ManagedSession* s=actor->exec.session; const FernManagedFunction* f=managed_function(s,frame); size_t cost=0;
    if(!f || !f->step || (f->mailbox && f->mailbox!=actor->mailbox)) { managed_fail(&actor->exec,FAULT_DESCRIPTOR); return false; }
    if(!managed_frame_cost(s,frame,&cost) || !managed_charge(s,cost)) { managed_fail(&actor->exec,FAULT_RESOURCE); return false; }
    managed_release(s,actor->frame_cost); actor->frame=frame; actor->frame_cost=cost;
    managed_clear_receive(actor); managed_enqueue(actor); return true;
}

/** Select the oldest eligible message; selectors run only pure pattern/guard code. @param actor Suspended owner. @return Selection or timeout published. */
static bool managed_poll(ManagedActor* actor,bool initial) {
    assert(actor!=NULL && actor->waiting);
    ManagedSession* s=actor->exec.session; const FernManagedFunction* selector=managed_function(s,actor->selector);
    assert(selector!=NULL && selector->select!=NULL);
    ManagedMessage* previous=NULL; ManagedMessage* message=actor->first;
    for(size_t i=0;i<MANAGED_MAILBOX && message;i++) {
        if(!initial && actor->deadline!=UINT64_MAX && message->enqueued>=actor->deadline) {
            previous=message; message=message->next; continue;
        }
        void* selected=selector->select(&actor->exec,actor->selector,message->value);
        if(actor->fault) return false;
        if(selected) {
            if(!managed_replace(actor,selected)) return false;
            if(previous) previous->next=message->next; else actor->first=message->next;
            if(actor->last==message) actor->last=previous;
            managed_release(s,message->cost); message->next=NULL; message->value=0; message->cost=0;
            assert(s->messages>0 && actor->messages>0); s->messages--; actor->messages--; return true;
        }
        previous=message; message=message->next;
    }
    if(actor->deadline!=UINT64_MAX) {
        uint64_t now=0; if(!managed_now(&now)) { managed_fail(&actor->exec,FAULT_CLOCK); return false; }
        if(now>=actor->deadline) return managed_replace(actor,actor->timeout_frame);
    }
    return false;
}

/** Publish a validated successor while retaining the same actor identity and fault slot. @param exec Owner. @param frame Successor. @return Scheduler status. */
int64_t fern_managed_continue(FernManagedExec* exec,void* frame) {
    assert(exec!=NULL);
    if(!exec->actor || !exec->actor->alive) { managed_fail(exec,FAULT_DESCRIPTOR); return FERN_MANAGED_FAILED; }
    return managed_replace(exec->actor,frame) ? FERN_MANAGED_RUNNABLE : FERN_MANAGED_FAILED;
}

/** Register one selector and timeout once; scheduler resumption never re-evaluates their expressions. */
int64_t fern_managed_receive(FernManagedExec* exec,void* selector,void* timeout,int64_t duration) {
    if(!exec || !exec->actor) { if(exec) managed_fail(exec,FAULT_DESCRIPTOR); return FERN_MANAGED_FAILED; }
    ManagedActor* actor=exec->actor; ManagedSession* s=exec->session;
    if(duration < -1 || duration>MANAGED_TIMEOUT || (duration>=0)!=(timeout!=NULL)) { managed_fail(exec,FAULT_TIMEOUT); return FERN_MANAGED_FAILED; }
    const FernManagedFunction* select=managed_function(s,selector); size_t select_cost=0, timeout_cost=0;
    const FernManagedFunction* after=timeout ? managed_function(s,timeout) : NULL;
    if(actor->waiting || !select || !select->select || select->mailbox!=actor->mailbox || (timeout && (!after || !after->step || (after->mailbox && after->mailbox!=actor->mailbox)))) { managed_fail(exec,FAULT_DESCRIPTOR); return FERN_MANAGED_FAILED; }
    uint64_t deadline=UINT64_MAX,now=0;
    if(duration>=0) {
        if(!managed_now(&now) || now>=UINT64_MAX-(uint64_t)duration) { managed_fail(exec,FAULT_CLOCK); return FERN_MANAGED_FAILED; }
        deadline=now+(uint64_t)duration;
    }
    if(!managed_frame_cost(s,selector,&select_cost) || (timeout && !managed_frame_cost(s,timeout,&timeout_cost)) || !managed_charge(s,select_cost+timeout_cost)) { managed_fail(exec,FAULT_RESOURCE); return FERN_MANAGED_FAILED; }
    actor->selector=selector; actor->selector_cost=select_cost; actor->timeout_frame=timeout; actor->timeout_cost=timeout_cost; actor->waiting=true;
    /* Selection/timeout closures now own all values that can survive this suspension. */
    managed_release(s,actor->frame_cost); actor->frame=NULL; actor->frame_cost=0;
    actor->deadline=deadline;
    if(deadline<s->next_deadline) s->next_deadline=deadline;
    bool selected=managed_poll(actor,true);
    if(actor->fault) return FERN_MANAGED_FAILED;
    return selected ? FERN_MANAGED_RUNNABLE : FERN_MANAGED_SUSPENDED;
}

/** Remove due receivers from their old message wake positions before sorting timer wakes.
 * @param s Invocation owner. @param now Checked monotonic milliseconds.
 */
static void managed_unqueue_due(ManagedSession* s,uint64_t now) {
    assert(s!=NULL);
    assert(s->live<=MANAGED_LIVE);
    ManagedActor* previous=NULL; ManagedActor* actor=s->first;
    for(size_t i=0;i<MANAGED_LIVE && actor;i++) {
        ManagedActor* next=actor->next;
        if(actor->waiting && actor->deadline!=UINT64_MAX && actor->deadline<=now) {
            if(previous) previous->next=next; else s->first=next;
            if(s->last==actor) s->last=previous;
            actor->queued=false; actor->next=NULL;
        } else previous=actor;
        actor=next;
    }
    assert(actor==NULL);
}

/** Promote elapsed timers in deadline/identity order at a cooperative scheduling boundary.
 * @param s Invocation owner. @param now Checked monotonic milliseconds.
 */
static void managed_wake_due(ManagedSession* s,uint64_t now) {
    assert(s!=NULL);
    assert(s->live<=MANAGED_LIVE);
    uint64_t earliest=UINT64_MAX;
    ManagedActor* due[MANAGED_LIVE]; size_t count=0;
    for(size_t i=0;i<s->next_id;i++) {
        ManagedActor* actor=s->identities[i];
        if(!actor->alive || !actor->waiting || actor->deadline==UINT64_MAX) continue;
        if(actor->deadline<=now) {
            assert(count<MANAGED_LIVE);
            size_t index=count++;
            while(index>0 && (due[index-1]->deadline>actor->deadline ||
                  (due[index-1]->deadline==actor->deadline && due[index-1]->id>actor->id))) {
                due[index]=due[index-1]; index--;
            }
            due[index]=actor;
        }
        else if(actor->deadline<earliest) earliest=actor->deadline;
    }
    if(count) managed_unqueue_due(s,now);
    /* Due timers are consumed below; pure selectors cannot register new timer deadlines. */
    s->next_deadline=earliest;
    for(size_t i=0;i<count;i++) {
        /* Publish the successor now: a later after-0 must not bypass a pending selector quantum. */
        if(!managed_poll(due[i],false) && !due[i]->fault && due[i]->deadline<earliest) {
            earliest=due[i]->deadline;
        }
        if(due[i]->fault) {
            if(!*s->root.fault) *s->root.fault=due[i]->fault;
            break;
        }
    }
    s->next_deadline=earliest;
}

/** Wake due receives and sleep only while no actor is runnable.
 * @param s Owner. @return Work can continue.
 */
static bool managed_idle(ManagedSession* s) {
    assert(s!=NULL && s->first==NULL);
    uint64_t now=0;
    if(!managed_now(&now)) { managed_fail(&s->root,FAULT_CLOCK); return false; }
    managed_wake_due(s,now);
    if(*s->root.fault) return false;
    if(s->first) return true;
    if(s->next_deadline==UINT64_MAX) { managed_fail(&s->root,FAULT_DEADLOCK); return false; }
    uint64_t delay=s->next_deadline-now;
    struct timespec sleep={(time_t)(delay/1000u),(long)(delay%1000u)*1000000L};
    if(nanosleep(&sleep,NULL)!=0 && errno!=EINTR) { managed_fail(&s->root,FAULT_CLOCK); return false; }
    return true;
}

/** Drive each actor through explicit frames; a failure stops the session after helper cleanup. */
void fern_managed_run(FernManagedExec* exec) {
    if(!exec || !exec->session) return;
    ManagedSession* s=exec->session;
    while(s->live && !*s->root.fault && !s->stopped) {
        /* Check only the cached earliest timer; scan identities once it becomes due. */
        if(s->next_deadline!=UINT64_MAX) {
            uint64_t now=0;
            if(!managed_now(&now)) { managed_fail(&s->root,FAULT_CLOCK); break; }
            if(now>=s->next_deadline) managed_wake_due(s,now);
            if(*s->root.fault) break;
        }
        ManagedActor* actor=managed_dequeue(s);
        if(!actor) { if(!managed_idle(s)) break; continue; }
        if(!actor->alive) continue;
        int64_t status=FERN_MANAGED_SUSPENDED;
        if(actor->waiting) { managed_poll(actor,false); }
        else {
            const FernManagedFunction* f=managed_function(s,actor->frame);
            assert(f!=NULL && f->step!=NULL);
            status=f->step(&actor->exec,actor->frame);
            if(status==FERN_MANAGED_COMPLETE) managed_finish(actor);
            else if(status!=FERN_MANAGED_RUNNABLE && status!=FERN_MANAGED_SUSPENDED && status!=FERN_MANAGED_FAILED) managed_fail(&actor->exec,FAULT_DESCRIPTOR);
            else if(status==FERN_MANAGED_RUNNABLE && !actor->queued) managed_fail(&actor->exec,FAULT_DESCRIPTOR);
            else if(status==FERN_MANAGED_SUSPENDED && !actor->waiting) managed_fail(&actor->exec,FAULT_DESCRIPTOR);
            else if(status==FERN_MANAGED_FAILED && !actor->fault) managed_fail(&actor->exec,FAULT_DESCRIPTOR);
        }
        if(actor->fault && !*s->root.fault) *s->root.fault=actor->fault;
    }
    if(*s->root.fault) fern_managed_stop(exec);
}

/** Clear only this invocation's roots; ordinary source helpers already drained their own cleanup. */
void fern_managed_stop(FernManagedExec* exec) {
    if(!exec || !exec->session) return;
    ManagedSession* s=exec->session; if(s->stopped) return;
    s->stopped=true; s->first=NULL; s->last=NULL; s->next_deadline=UINT64_MAX;
    for(size_t i=0;i<s->next_id;i++) { ManagedActor* a=s->identities[i]; a->queued=false; a->next=NULL; managed_finish(a); }
}
