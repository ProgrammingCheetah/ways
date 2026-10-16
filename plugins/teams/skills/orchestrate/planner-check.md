# The planner check

Run around every `teams:planner` run (lead, QA, each resume), in its planning workspace `<ws>`. It works the same in git, in jj (including secondary workspaces, which have no `.git`), and with no VCS at all.

**Before the run**

1. Confirm `<ws>` is clean: `git status --porcelain --ignored` prints nothing (git), or `jj st` reports no changes (jj). Not clean: recreate the workspace at the base branch's tip.
2. Create an empty marker file outside every repo, e.g. `<workspaces folder>/.scratch/planner-<team>-<run>.mark`. Its modification time is the line the run must not cross.

**After the run**

3. List everything in `<ws>` modified after the marker, outside `.git` and `.jj`. Directories count: adding or deleting a file changes its directory's time, so this catches deletions and ignored files too.
   - bash or zsh: `find <ws> -newer <mark> -not -path '*/.git/*' -not -path '*/.jj/*' -not -name .git -not -name .jj`
   - PowerShell: `$t=(Get-Item <mark>).LastWriteTime; Get-ChildItem <ws> -Recurse -Force | Where-Object { $_.LastWriteTime -gt $t -and $_.FullName -notmatch '[\\/]\.(git|jj)([\\/]|$)' }`
4. Repeat step 1's status command.

**On any change** (step 3 lists anything, or step 4 isn't clean): stop the slice, show the Engineer what changed (the list, and `git diff` or `jj diff`), don't save the plan, and recreate the workspace before the next run. Writes outside `<ws>` are not detected.
