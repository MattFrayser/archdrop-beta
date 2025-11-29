# ArchDrop Code Quality Audit & Refactoring Report

## CRITICAL BUGS 🔴

### 1. TUI Progress Bar Not Working
**Location:** `src/ui/tui.rs:46`, `src/server/mod.rs`
**Issue:** The `progress_sender` channel is created but never used to send progress updates
**Impact:** Progress bar stays at 0%, users have no feedback on transfer progress
**Root Cause:** Send/receive handlers don't call `progress_sender.send()`

### 2. TUI Never Closes After Session Ends
**Location:** `src/ui/tui.rs:62-64`
**Issue:** TUI waits for `progress >= 100.0` which never happens
**Impact:** Users must manually press 'q' to exit, poor UX
**Root Cause:** Progress is never updated, so loop never breaks

---

## DRY VIOLATIONS (Code Duplication) 🟡

### JavaScript Duplication

#### 1. `formatFileSize()` Function
**Files:**
- `templates/upload/upload.js:322-328` (7 lines)
- `templates/download/download.js:81-87` (7 lines)

**Duplication:** 100% identical code
**Fix:** Move to `templates/shared/crypto.js`

#### 2. `runWithConcurrency()` Function
**Files:**
- `templates/upload/upload.js:331-349` (19 lines)
- `templates/download/download.js:265-283` (19 lines)

**Duplication:** 100% identical code
**Fix:** Move to `templates/shared/crypto.js`

#### 3. `createFileItem()` Function
**Files:**
- `templates/upload/upload.js:42-99` (58 lines)
- `templates/download/download.js:37-79` (43 lines)

**Duplication:** ~80% similar (only differs in remove button vs no button)
**Fix:** Create shared version with optional parameter for remove button

### HTML/CSS Duplication

#### 4. Shared CSS Styles (200+ lines duplicated)
**Files:** `upload.html` and `download.html`

**Duplicated CSS Classes:**
- `.file-list` - Scrollable container styles
- `.file-item` - File card styles
- `.file-icon`, `.file-details`, `.file-name`, `.file-size`
- `.file-progress`, `.progress-bar-container`, `.progress-bar`, `.progress-text`
- `.info`, `.info-item` - Information section styles
- Body background, container, subtitle, etc.

**Total Duplication:** ~220 lines of identical CSS
**Fix:** Extract to `templates/shared/styles.css`

---

## YAGNI VIOLATIONS (Unused Code) 🟠

### 1. Unused Rust Files
- **`src/crypto/stream.rs`** (72 lines) - `EncryptedFileStream` never used
- **`src/crypto/encrypt.rs`** (55 lines) - `Encryptor` struct never used

**Impact:** Dead code increases maintenance burden
**Fix:** Delete both files

### 2. Unused JavaScript Functions
- **`crypto.js:91-123`** - `createFrame()`, `parseFrames()` (33 lines)
- **`upload.js:220-233`** - `getCompletedChunks()` (14 lines)

**Impact:** Confusing for developers, suggests features that don't exist
**Fix:** Remove unused functions

---

## CODE QUALITY ISSUES 🟢

### 1. Typos
- `src/ui/tui.rs:19` - "is_recieving" → "is_receiving"
- `templates/shared/crypto.js:71` - "immediatly" → "immediately"
- `templates/shared/crypto.js:110` - "encrpted" → "encrypted"

### 2. Inconsistent Terminology
**Problem:** Mixed use of "send"/"receive" vs "upload"/"download"
- Routes use `/send/` and `/receive/`
- But checking `service == "upload"` in modes.rs:65, 142
- Variable names inconsistent

**Fix:** Standardize on "send"/"receive" throughout

### 3. No CSS Separation
**Problem:** All CSS inline in HTML files
**Impact:** Difficult to maintain, poor separation of concerns
**Fix:** Extract to shared CSS file

### 4. Magic Strings
**Examples:**
- `"send"`, `"receive"`, `"upload"` as string literals throughout code
- Should use constants or enums

---

## REFACTORING PLAN

### Phase 1: Fix Critical Bugs ⚡
1. ✅ Add progress tracking to send handler
2. ✅ Add progress tracking to receive handler
3. ✅ Update TUI to close on session complete signal

### Phase 2: Remove Dead Code 🗑️
1. ✅ Delete `src/crypto/stream.rs`
2. ✅ Delete `src/crypto/encrypt.rs`
3. ✅ Remove unused JS functions
4. ✅ Update module exports

### Phase 3: Extract Shared Code 📦
1. ✅ Move `formatFileSize()` to shared utilities
2. ✅ Move `runWithConcurrency()` to shared utilities
3. ✅ Create unified `createFileItem()` function
4. ✅ Extract shared CSS to `templates/shared/styles.css`
5. ✅ Update HTML files to include shared CSS

### Phase 4: Code Quality Improvements ✨
1. ✅ Fix all typos
2. ✅ Standardize terminology (send/receive)
3. ✅ Add constants for route names
4. ✅ Improve comments

---

## ESTIMATED IMPACT

### Before Refactoring:
- **Total Lines:** ~3,500
- **Duplicated Code:** ~350 lines
- **Unused Code:** ~180 lines
- **Bugs:** 2 critical

### After Refactoring:
- **Total Lines:** ~3,000 (-14%)
- **Duplicated Code:** 0
- **Unused Code:** 0
- **Bugs:** 0

### Benefits:
- ✅ Working progress bars
- ✅ TUI auto-closes
- ✅ Easier to maintain (DRY)
- ✅ Smaller codebase (removed 500+ lines)
- ✅ Better separation of concerns
- ✅ Production-ready code quality
