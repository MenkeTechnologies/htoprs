//! Stub scaffold for `Action.c` — NOT yet ported.
//!
//! Every `pub fn` below is a placeholder (`todo!()`) named after a real
//! htop C function so the port-purity gate accepts the module and the
//! port surface is laid out. Replace each stub with a faithful port of
//! the C body, updating the signature and the doc comment to `Port of
//! `Action.c`:<line>.` as you go. `gen_port_report.py` counts these
//! `todo!()` bodies as *stubbed*, not *ported*, so scaffolding does not
//! inflate coverage.
#![allow(non_snake_case)]
#![allow(dead_code)]


/// TODO: port of `Object* Action_pickFromVector(State* st, Panel* list, int x, bool follow` from `Action.c:59`.
pub fn Action_pickFromVector() {
    todo!("port of Action.c:59")
}

/// TODO: port of `static void Action_runSetup(State* st` from `Action.c:101`.
pub fn Action_runSetup() {
    todo!("port of Action.c:101")
}

/// TODO: port of `static bool changePriority(MainPanel* panel, int delta` from `Action.c:113`.
pub fn changePriority() {
    todo!("port of Action.c:113")
}

/// TODO: port of `static void addUserToVector(ht_key_t key, void* userCast, void* panelCast` from `Action.c:121`.
pub fn addUserToVector() {
    todo!("port of Action.c:121")
}

/// TODO: port of `bool Action_setUserOnly(const char* userName, uid_t* userId` from `Action.c:127`.
pub fn Action_setUserOnly() {
    todo!("port of Action.c:127")
}

/// TODO: port of `static void tagAllChildren(Panel* panel, Row* parent` from `Action.c:137`.
pub fn tagAllChildren() {
    todo!("port of Action.c:137")
}

/// TODO: port of `static bool expandCollapse(Panel* panel` from `Action.c:148`.
pub fn expandCollapse() {
    todo!("port of Action.c:148")
}

/// TODO: port of `static bool collapseIntoParent(Panel* panel` from `Action.c:157`.
pub fn collapseIntoParent() {
    todo!("port of Action.c:157")
}

/// TODO: port of `Htop_Reaction Action_setSortKey(Settings* settings, ProcessField sortKey` from `Action.c:174`.
pub fn Action_setSortKey() {
    todo!("port of Action.c:174")
}

/// TODO: port of `static bool Action_writeableProcess(State* st` from `Action.c:181`.
pub fn Action_writeableProcess() {
    todo!("port of Action.c:181")
}

/// TODO: port of `static bool Action_readableProcess(State* st` from `Action.c:187`.
pub fn Action_readableProcess() {
    todo!("port of Action.c:187")
}

/// TODO: port of `static Htop_Reaction actionSetSortColumn(State* st` from `Action.c:192`.
pub fn actionSetSortColumn() {
    todo!("port of Action.c:192")
}

/// TODO: port of `static Htop_Reaction actionSortByPID(State* st` from `Action.c:227`.
pub fn actionSortByPID() {
    todo!("port of Action.c:227")
}

/// TODO: port of `static Htop_Reaction actionSortByMemory(State* st` from `Action.c:231`.
pub fn actionSortByMemory() {
    todo!("port of Action.c:231")
}

/// TODO: port of `static Htop_Reaction actionSortByCPU(State* st` from `Action.c:235`.
pub fn actionSortByCPU() {
    todo!("port of Action.c:235")
}

/// TODO: port of `static Htop_Reaction actionSortByTime(State* st` from `Action.c:239`.
pub fn actionSortByTime() {
    todo!("port of Action.c:239")
}

/// TODO: port of `static Htop_Reaction actionToggleKernelThreads(State* st` from `Action.c:243`.
pub fn actionToggleKernelThreads() {
    todo!("port of Action.c:243")
}

/// TODO: port of `static Htop_Reaction actionToggleUserlandThreads(State* st` from `Action.c:253`.
pub fn actionToggleUserlandThreads() {
    todo!("port of Action.c:253")
}

/// TODO: port of `static Htop_Reaction actionToggleRunningInContainer(State* st` from `Action.c:263`.
pub fn actionToggleRunningInContainer() {
    todo!("port of Action.c:263")
}

/// TODO: port of `static Htop_Reaction actionToggleProgramPath(State* st` from `Action.c:271`.
pub fn actionToggleProgramPath() {
    todo!("port of Action.c:271")
}

/// TODO: port of `static Htop_Reaction actionToggleMergedCommand(State* st` from `Action.c:279`.
pub fn actionToggleMergedCommand() {
    todo!("port of Action.c:279")
}

/// TODO: port of `static Htop_Reaction actionToggleTreeView(State* st` from `Action.c:287`.
pub fn actionToggleTreeView() {
    todo!("port of Action.c:287")
}

/// TODO: port of `static Htop_Reaction actionToggleHideMeters(State* st` from `Action.c:300`.
pub fn actionToggleHideMeters() {
    todo!("port of Action.c:300")
}

/// TODO: port of `static Htop_Reaction actionExpandOrCollapseAllBranches(State* st` from `Action.c:305`.
pub fn actionExpandOrCollapseAllBranches() {
    todo!("port of Action.c:305")
}

/// TODO: port of `static Htop_Reaction actionIncFilter(State* st` from `Action.c:319`.
pub fn actionIncFilter() {
    todo!("port of Action.c:319")
}

/// TODO: port of `static Htop_Reaction actionIncSearch(State* st` from `Action.c:327`.
pub fn actionIncSearch() {
    todo!("port of Action.c:327")
}

/// TODO: port of `static Htop_Reaction actionHigherPriority(State* st` from `Action.c:333`.
pub fn actionHigherPriority() {
    todo!("port of Action.c:333")
}

/// TODO: port of `static Htop_Reaction actionLowerPriority(State* st` from `Action.c:341`.
pub fn actionLowerPriority() {
    todo!("port of Action.c:341")
}

/// TODO: port of `static Htop_Reaction actionInvertSortOrder(State* st` from `Action.c:349`.
pub fn actionInvertSortOrder() {
    todo!("port of Action.c:349")
}

/// TODO: port of `static Htop_Reaction actionExpandOrCollapse(State* st` from `Action.c:356`.
pub fn actionExpandOrCollapse() {
    todo!("port of Action.c:356")
}

/// TODO: port of `static Htop_Reaction actionCollapseIntoParent(State* st` from `Action.c:364`.
pub fn actionCollapseIntoParent() {
    todo!("port of Action.c:364")
}

/// TODO: port of `static Htop_Reaction actionExpandCollapseOrSortColumn(State* st` from `Action.c:372`.
pub fn actionExpandCollapseOrSortColumn() {
    todo!("port of Action.c:372")
}

/// TODO: port of `static inline void setActiveScreen(Settings* settings, State* st, unsigned int ssIdx` from `Action.c:376`.
pub fn setActiveScreen() {
    todo!("port of Action.c:376")
}

/// TODO: port of `static Htop_Reaction actionNextScreen(State* st` from `Action.c:390`.
pub fn actionNextScreen() {
    todo!("port of Action.c:390")
}

/// TODO: port of `static Htop_Reaction actionPrevScreen(State* st` from `Action.c:400`.
pub fn actionPrevScreen() {
    todo!("port of Action.c:400")
}

/// TODO: port of `Htop_Reaction Action_setScreenTab(State* st, int x` from `Action.c:411`.
pub fn Action_setScreenTab() {
    todo!("port of Action.c:411")
}

/// TODO: port of `static Htop_Reaction actionQuit(ATTR_UNUSED State* st` from `Action.c:439`.
pub fn actionQuit() {
    todo!("port of Action.c:439")
}

/// TODO: port of `static Htop_Reaction actionSetAffinity(State* st` from `Action.c:443`.
pub fn actionSetAffinity() {
    todo!("port of Action.c:443")
}

/// TODO: port of `static Htop_Reaction actionSetSchedPolicy(State* st` from `Action.c:480`.
pub fn actionSetSchedPolicy() {
    todo!("port of Action.c:480")
}

/// TODO: port of `static Htop_Reaction actionKill(State* st` from `Action.c:524`.
pub fn actionKill() {
    todo!("port of Action.c:524")
}

/// TODO: port of `static Htop_Reaction actionFilterByUser(State* st` from `Action.c:548`.
pub fn actionFilterByUser() {
    todo!("port of Action.c:548")
}

/// TODO: port of `Htop_Reaction Action_follow(State* st` from `Action.c:568`.
pub fn Action_follow() {
    todo!("port of Action.c:568")
}

/// TODO: port of `static Htop_Reaction actionSetup(State* st` from `Action.c:574`.
pub fn actionSetup() {
    todo!("port of Action.c:574")
}

/// TODO: port of `static Htop_Reaction actionLsof(State* st` from `Action.c:579`.
pub fn actionLsof() {
    todo!("port of Action.c:579")
}

/// TODO: port of `static Htop_Reaction actionShowLocks(State* st` from `Action.c:597`.
pub fn actionShowLocks() {
    todo!("port of Action.c:597")
}

/// TODO: port of `static Htop_Reaction actionBacktrace(State *st` from `Action.c:616`.
pub fn actionBacktrace() {
    todo!("port of Action.c:616")
}

/// TODO: port of `static Htop_Reaction actionStrace(State* st` from `Action.c:644`.
pub fn actionStrace() {
    todo!("port of Action.c:644")
}

/// TODO: port of `static Htop_Reaction actionTag(State* st` from `Action.c:665`.
pub fn actionTag() {
    todo!("port of Action.c:665")
}

/// TODO: port of `static Htop_Reaction actionRedraw(ATTR_UNUSED State* st` from `Action.c:675`.
pub fn actionRedraw() {
    todo!("port of Action.c:675")
}

/// TODO: port of `static Htop_Reaction actionTogglePauseUpdate(State* st` from `Action.c:681`.
pub fn actionTogglePauseUpdate() {
    todo!("port of Action.c:681")
}

/// TODO: port of `static inline void addattrstr( int attr, const char* str` from `Action.c:746`.
pub fn addattrstr() {
    todo!("port of Action.c:746")
}

/// TODO: port of `static Htop_Reaction actionHelp(State* st` from `Action.c:751`.
pub fn actionHelp() {
    todo!("port of Action.c:751")
}

/// TODO: port of `static Htop_Reaction actionUntagAll(State* st` from `Action.c:894`.
pub fn actionUntagAll() {
    todo!("port of Action.c:894")
}

/// TODO: port of `static Htop_Reaction actionTagAllChildren(State* st` from `Action.c:902`.
pub fn actionTagAllChildren() {
    todo!("port of Action.c:902")
}

/// TODO: port of `static Htop_Reaction actionShowEnvScreen(State* st` from `Action.c:911`.
pub fn actionShowEnvScreen() {
    todo!("port of Action.c:911")
}

/// TODO: port of `static Htop_Reaction actionShowCommandScreen(State* st` from `Action.c:929`.
pub fn actionShowCommandScreen() {
    todo!("port of Action.c:929")
}

/// TODO: port of `void Action_setBindings(Htop_Action* keys` from `Action.c:947`.
pub fn Action_setBindings() {
    todo!("port of Action.c:947")
}
