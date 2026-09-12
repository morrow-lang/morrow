/** Private scheduler invariants are tested independently of generated source and optimization. */
#define _POSIX_C_SOURCE 200809L
#include <time.h>
static int actor_test_clock(clockid_t,struct timespec*);
#define clock_gettime actor_test_clock
#include "../../runtime/fern_managed.c"
#undef clock_gettime
#include <stdio.h>
#undef assert
#define assert(condition) do { if(!(condition)) { fprintf(stderr,"check failed at line %d: %s\n",__LINE__,#condition); exit(1); } } while(0)
static void* allocations[400000];
static size_t allocation_count;
static unsigned executed;
static bool fake_clock, fail_clock, advance_allocation, fail_allocation_clock;
static uint64_t fake_milliseconds;
/** Deterministic clock seam retains real monotonic time outside the exact commit tests. */
static int actor_test_clock(clockid_t id,struct timespec* output) {
    if(!fake_clock) return clock_gettime(id,output);
    if(fail_clock) return -1;
    output->tv_sec=(time_t)(fake_milliseconds/1000u);
    output->tv_nsec=(long)(fake_milliseconds%1000u)*1000000L;
    return 0;
}
static FernManagedType scalar={FERN_MANAGED_SCALAR,0,NULL,NULL};
/** Retain test heap objects until the bounded scenario completes. */
void* fern_alloc(size_t size) {
    assert(allocation_count<400000);
    void* value=calloc(1,size); assert(value!=NULL);
    allocations[allocation_count++]=value;
    if(size==sizeof(ManagedMessage) && advance_allocation) {
        fake_milliseconds+=1000; advance_allocation=false;
        if(fail_allocation_clock) fail_clock=true;
    }
    return value;
}
int64_t fern_result_ok(int64_t value) { int64_t* result=fern_alloc(16); result[0]=0; result[1]=value; return (int64_t)(intptr_t)result; }
int64_t fern_result_err(int64_t value) { int64_t* result=fern_alloc(16); result[0]=1; result[1]=value; return (int64_t)(intptr_t)result; }
static int64_t done(FernManagedExec* exec,void* env) { (void)exec; (void)env; executed++; return FERN_MANAGED_COMPLETE; }
static int64_t fail_first(FernManagedExec* exec,void* env) { (void)env; *fern_managed_fault(exec)=1; return FERN_MANAGED_FAILED; }
static void* unmatched(FernManagedExec* exec,void* env,int64_t value) { (void)exec; (void)env; (void)value; return NULL; }
static FernManagedFunction done_descriptor={(void*)done,done,NULL,0,NULL,&scalar};
static FernManagedFunction failure_descriptor={(void*)fail_first,fail_first,NULL,0,NULL,&scalar};
static FernManagedFunction selector_descriptor={(void*)unmatched,NULL,unmatched,0,NULL,&scalar};
static const FernManagedFunction* descriptors[]={&done_descriptor,&failure_descriptor,&selector_descriptor};
static void* frame(const void* code) { const void** value=fern_alloc(8); *value=code; return value; }
static FernManagedExec* context(int64_t* fault) { return fern_managed_new(fault,descriptors,3); }

/** A suspended frame retains only selectors and live continuation captures, not its spent entry. */
static void suspension_roots(void) {
    int64_t fault=0; FernManagedExec* root=context(&fault);
    ManagedPid* pid=fern_managed_spawn(root,frame((void*)done),&scalar);
    ManagedActor* actor=managed_dequeue(root->session); assert(actor==pid->actor);
    void* selector=frame((void*)unmatched); void* timeout=frame((void*)done);
    int64_t status=fern_managed_receive(&actor->exec,selector,timeout,600000);
    assert(status==FERN_MANAGED_SUSPENDED && fault==0);
    assert(actor->frame==NULL && actor->frame_cost==0);
    assert(actor->selector==selector && actor->timeout_frame==timeout);
    fern_managed_stop(root);
    assert(!actor->alive && !actor->waiting && actor->first==NULL && actor->last==NULL);
    assert(actor->selector==NULL && actor->timeout_frame==NULL && actor->deadline==UINT64_MAX);
}

/** A prior actor fault wins and all pending actor/message roots are cleared before return. */
static void failure_precedence(void) {
    int64_t fault=0; FernManagedExec* root=context(&fault); executed=0;
    fern_managed_spawn(root,frame((void*)fail_first),&scalar);
    ManagedPid* pid=fern_managed_spawn(root,frame((void*)done),&scalar);
    fern_managed_send(root,pid,42,&scalar); fern_managed_run(root);
    assert(fault==1 && executed==0 && root->session->live==0 && root->session->messages==0);
    assert(root->session->first==NULL && root->session->last==NULL);
    assert(pid->actor->frame==NULL && pid->actor->first==NULL);
}

/** Failed enqueue and foreign identity checks cannot mutate the receiver's ordered mailbox. */
static void mailbox_atomicity(void) {
    int64_t fault=0,other_fault=0; FernManagedExec* root=context(&fault); FernManagedExec* other=context(&other_fault);
    ManagedPid* pid=fern_managed_spawn(root,frame((void*)done),&scalar);
    for(size_t i=0;i<MANAGED_MAILBOX;i++) { int64_t result=fern_managed_send(root,pid,(int64_t)i,&scalar); assert(((int64_t*)(intptr_t)result)[0]==0); }
    ManagedMessage* last=pid->actor->last; size_t retained=root->session->retained;
    int64_t* full=(int64_t*)(intptr_t)fern_managed_send(root,pid,999,&scalar);
    assert(full[0]==1 && full[1]==4 && pid->actor->last==last && root->session->retained==retained);
    int64_t* foreign=(int64_t*)(intptr_t)fern_managed_send(other,pid,123,&scalar);
    assert(foreign[0]==1 && foreign[1]==3 && pid->actor->messages==MANAGED_MAILBOX);
    fern_managed_stop(root); fern_managed_stop(other); assert(fault==0 && other_fault==0);
}

/** Descriptor metadata shares one invocation preflight budget, even when each field is trivial. */
static void descriptor_aggregate_budget(void) {
    const size_t count=300,fields=4096;
    const FernManagedType** captures=fern_alloc(fields*sizeof(*captures));
    for(size_t i=0;i<fields;i++) captures[i]=&scalar;
    FernManagedFunction* entries=fern_alloc(count*sizeof(*entries));
    const FernManagedFunction** table=fern_alloc(count*sizeof(*table));
    for(size_t i=0;i<count;i++) {
        entries[i]=(FernManagedFunction){&entries[i],done,NULL,(int64_t)fields,captures,&scalar};
        table[i]=&entries[i];
    }
    size_t before=allocation_count; int64_t fault=0;
    FernManagedExec* root=fern_managed_new(&fault,table,(int64_t)count);
    assert(root==NULL && fault==FAULT_DESCRIPTOR && allocation_count==before);
}

/** A PID payload must retain only its own session and its exact mailbox capability type. */
static void pid_graph_provenance(void) {
    int64_t first_fault=0,second_fault=0;
    FernManagedExec* first=context(&first_fault); FernManagedExec* second=context(&second_fault);
    ManagedPid* local=fern_managed_spawn(first,frame((void*)done),&scalar);
    ManagedPid* foreign=fern_managed_spawn(second,frame((void*)done),&scalar);
    const FernManagedType* children[]={&scalar};
    FernManagedType pid_type={FERN_MANAGED_PID,1,children,NULL}; size_t bytes=0;
    assert(managed_value_cost(first->session,&pid_type,(int64_t)(intptr_t)local,&bytes));
    assert(!managed_value_cost(first->session,&pid_type,(int64_t)(intptr_t)foreign,&bytes));
    fern_managed_stop(first); fern_managed_stop(second);
    assert(managed_value_cost(first->session,&pid_type,(int64_t)(intptr_t)local,&bytes));
}

/** Overdue timers are ordered by deadline, then stable actor identity, after a blocking quantum. */
static void overdue_timer_order(void) {
    int64_t fault=0; FernManagedExec* root=context(&fault);
    fern_managed_spawn(root,frame((void*)done),&scalar);
    fern_managed_spawn(root,frame((void*)done),&scalar);
    ManagedActor* first=managed_dequeue(root->session); ManagedActor* second=managed_dequeue(root->session);
    assert(fern_managed_receive(&first->exec,frame((void*)unmatched),frame((void*)done),600000)==FERN_MANAGED_SUSPENDED);
    assert(fern_managed_receive(&second->exec,frame((void*)unmatched),frame((void*)done),600000)==FERN_MANAGED_SUSPENDED);
    uint64_t now=0; assert(managed_now(&now) && now>=2);
    first->deadline=now-1; second->deadline=now-2;
    assert(managed_idle(root->session));
    assert(managed_dequeue(root->session)==second);
    assert(managed_dequeue(root->session)==first);
    /* Promotion now installs ready successors, so arm fresh receives for the tie case. */
    assert(!first->waiting && !second->waiting);
    assert(fern_managed_receive(&first->exec,frame((void*)unmatched),frame((void*)done),600000)==FERN_MANAGED_SUSPENDED);
    assert(fern_managed_receive(&second->exec,frame((void*)unmatched),frame((void*)done),600000)==FERN_MANAGED_SUSPENDED);
    first->deadline=now; second->deadline=now;
    assert(managed_idle(root->session));
    assert(managed_dequeue(root->session)==first);
    assert(managed_dequeue(root->session)==second);
    fern_managed_stop(root); assert(fault==0);
}

/** String scanning spends its remaining work before reading each bounded text chunk. */
static void string_scan_budget(void) {
    int64_t fault=0; FernManagedExec* root=context(&fault);
    FernManagedType text_type={FERN_MANAGED_STRING,0,NULL,NULL};
    char text[129]; memset(text,'x',128); text[128]=0;
    ManagedSeen seen[MANAGED_GRAPH]={0};
    ManagedCost cost={root->session,seen,0,MANAGED_WORK-3,0};
    assert(!managed_cost_value(&cost,&text_type,(int64_t)(intptr_t)text,0));
    assert(fault==0); fern_managed_stop(root);
}

/** Metadata reads and pending-child copies consume the same budget before access. */
static void descriptor_edge_budget(void) {
    int64_t arities[4096]={0};
    FernManagedType sum={FERN_MANAGED_SUM,4096,NULL,arities};
    size_t work=MANAGED_WORK-2;
    assert(!managed_descriptor_walk(&sum,false,&work));
    const FernManagedType* children[4096];
    for(size_t i=0;i<4096;i++) children[i]=&scalar;
    FernManagedType product={FERN_MANAGED_PRODUCT,4096,children,NULL};
    work=MANAGED_WORK-2;
    assert(!managed_descriptor_walk(&product,false,&work));
}

/** Reject a wrong timeout effect before any roots or retained charge are published. */
static void timeout_descriptor_atomicity(void) {
    FernManagedType text={FERN_MANAGED_STRING,0,NULL,NULL};
    FernManagedFunction wrong={&text,done,NULL,0,NULL,&text};
    const FernManagedFunction* table[]={&done_descriptor,&selector_descriptor,&wrong};
    int64_t fault=0; FernManagedExec* root=fern_managed_new(&fault,table,3);
    ManagedPid* pid=fern_managed_spawn(root,frame((void*)done),&scalar);
    ManagedActor* actor=managed_dequeue(root->session); assert(actor==pid->actor);
    void* entry=actor->frame; size_t retained=root->session->retained;
    int64_t status=fern_managed_receive(&actor->exec,frame((void*)unmatched),frame(&text),600000);
    assert(status==FERN_MANAGED_FAILED && actor->fault==FAULT_DESCRIPTOR);
    assert(actor->frame==entry && !actor->waiting && actor->selector==NULL && actor->timeout_frame==NULL);
    assert(root->session->retained==retained);
    fern_managed_stop(root);
}

/** Capture identity lookup consumes the graph operation's remaining work allowance. */
static void identity_lookup_budget(void) {
    int64_t fault=0; FernManagedExec* root=context(&fault);
    ManagedSeen seen[MANAGED_GRAPH]={0};
    ManagedCost cost={root->session,seen,0,MANAGED_WORK-1,0};
    assert(!managed_cost_frame(&cost,frame((void*)unmatched),0));
    fern_managed_stop(root);
}

/** A selector returns an ordinary step only after an eligible candidate is presented. */
static void* select_any(FernManagedExec* exec,void* env,int64_t value) {
    (void)exec; (void)env; (void)value; return frame((void*)done);
}

/** Late messages cannot win after a registered absolute deadline has passed. */
static void late_message_deadline(void) {
    FernManagedFunction select={(void*)select_any,NULL,select_any,0,NULL,&scalar};
    const FernManagedFunction* table[]={&done_descriptor,&failure_descriptor,&select};
    int64_t fault=0; FernManagedExec* root=fern_managed_new(&fault,table,3);
    ManagedPid* pid=fern_managed_spawn(root,frame((void*)done),&scalar);
    ManagedActor* actor=managed_dequeue(root->session);
    void* timeout=frame((void*)fail_first);
    assert(fern_managed_receive(&actor->exec,frame((void*)select_any),timeout,600000)==FERN_MANAGED_SUSPENDED);
    uint64_t now=0; assert(managed_now(&now) && now>0); actor->deadline=now-1;
    fern_managed_send(root,pid,42,&scalar);
    assert(managed_poll(actor,false));
    assert(actor->frame==timeout && actor->messages==1);
    fern_managed_stop(root);
}

/** Timely queued messages still win when another actor delays the receiver's next quantum. */
static void timely_message_deadline(void) {
    FernManagedFunction select={(void*)select_any,NULL,select_any,0,NULL,&scalar};
    const FernManagedFunction* table[]={&done_descriptor,&failure_descriptor,&select};
    int64_t fault=0; FernManagedExec* root=fern_managed_new(&fault,table,3);
    ManagedPid* pid=fern_managed_spawn(root,frame((void*)done),&scalar);
    ManagedActor* actor=managed_dequeue(root->session); void* timeout=frame((void*)fail_first);
    assert(fern_managed_receive(&actor->exec,frame((void*)select_any),timeout,600000)==FERN_MANAGED_SUSPENDED);
    fern_managed_send(root,pid,42,&scalar);
    uint64_t now=0; assert(managed_now(&now) && now>2);
    actor->deadline=now-1; actor->first->enqueued=now-2;
    assert(managed_poll(actor,false));
    assert(actor->frame!=timeout && actor->messages==0);
    fern_managed_stop(root);
}

/** Enqueue time is sampled after potentially expensive graph validation and allocation. */
static void enqueue_commit_clock(void) {
    int64_t fault=0; FernManagedExec* root=context(&fault);
    ManagedPid* pid=fern_managed_spawn(root,frame((void*)done),&scalar);
    fake_clock=true; fake_milliseconds=1000; advance_allocation=true;
    int64_t* result=(int64_t*)(intptr_t)fern_managed_send(root,pid,42,&scalar);
    assert(result[0]==0 && pid->actor->first->enqueued==2000);
    size_t retained=root->session->retained; ManagedMessage* last=pid->actor->last;
    advance_allocation=true; fail_allocation_clock=true;
    result=(int64_t*)(intptr_t)fern_managed_send(root,pid,43,&scalar);
    assert(result[0]==1 && fault==FAULT_CLOCK);
    assert(root->session->retained==retained && pid->actor->last==last && last->next==NULL);
    fake_clock=false; fail_clock=false; fail_allocation_clock=false;
    fern_managed_stop(root);
}

static unsigned timer_step, timer_observed;
static void* timer_spin_frame;

/** Every continuation yields; an elapsed timer must not wait for this actor to complete. */
static int64_t timer_spin(FernManagedExec* exec,void* env) {
    (void)env;
    fake_milliseconds++;
    if(++timer_step==20) return FERN_MANAGED_COMPLETE;
    return fern_managed_continue(exec,timer_spin_frame);
}

/** Record the quantum at which the timeout continuation actually runs. */
static int64_t timer_record(FernManagedExec* exec,void* env) {
    (void)exec; (void)env;
    timer_observed=timer_step;
    return FERN_MANAGED_COMPLETE;
}

/** Runnable actors cannot starve an elapsed timer while repeatedly yielding continuations. */
static void timer_fairness_with_runnable_actor(void) {
    fake_clock=true; fake_milliseconds=0; timer_step=0; timer_observed=0;
    FernManagedFunction spin={(void*)timer_spin,timer_spin,NULL,0,NULL,&scalar};
    FernManagedFunction record={(void*)timer_record,timer_record,NULL,0,NULL,&scalar};
    const FernManagedFunction* table[]={&done_descriptor,&selector_descriptor,&spin,&record};
    int64_t fault=0; FernManagedExec* root=fern_managed_new(&fault,table,4);
    fern_managed_spawn(root,frame((void*)done),&scalar);
    ManagedActor* receiver=managed_dequeue(root->session);
    assert(fern_managed_receive(&receiver->exec,frame((void*)unmatched),
        frame((void*)timer_record),3)==FERN_MANAGED_SUSPENDED);
    timer_spin_frame=frame((void*)timer_spin);
    fern_managed_spawn(root,timer_spin_frame,&scalar);
    fern_managed_run(root);
    assert(fault==0 && timer_step==20);
    assert(timer_observed>=3 && timer_observed<=5);
    fern_managed_stop(root); fake_clock=false;
}

/** A selected zero-duration timeout is retired before its runnable successor is dispatched. */
static void retired_timer_does_not_read_clock(void) {
    fake_clock=true; fake_milliseconds=0;
    int64_t fault=0; FernManagedExec* root=context(&fault);
    fern_managed_spawn(root,frame((void*)done),&scalar);
    ManagedActor* actor=managed_dequeue(root->session);
    assert(fern_managed_receive(&actor->exec,frame((void*)unmatched),
        frame((void*)done),0)==FERN_MANAGED_RUNNABLE);
    assert(!actor->waiting && actor->queued);
    fail_clock=true;
    fern_managed_run(root);
    assert(fault==0 && root->session->live==0);
    fern_managed_stop(root); fail_clock=false; fake_clock=false;
}

int main(void) {
    retired_timer_does_not_read_clock(); timer_fairness_with_runnable_actor();
    enqueue_commit_clock(); timely_message_deadline(); late_message_deadline(); descriptor_edge_budget(); timeout_descriptor_atomicity(); identity_lookup_budget();
    string_scan_budget(); overdue_timer_order(); pid_graph_provenance(); descriptor_aggregate_budget(); suspension_roots(); failure_precedence(); mailbox_atomicity();
    for(size_t i=0;i<allocation_count;i++) free(allocations[i]);
    puts("managed ownership/first-fault/mailbox atomicity: ok"); return 0;
}
