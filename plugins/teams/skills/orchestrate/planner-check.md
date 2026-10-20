# The planner check

Run around every `teams:planner` run (lead, each resume) and every plan security challenge, on Claude or on a runner (`runners.md` adds its run folder), in its planning workspace `<ws>`. It works the same in git, in jj (including secondary workspaces, which have no `.git`), and with no VCS at all.

**Before the run**

1. Run `git status --porcelain --ignored` (git) or `jj st` (jj) in `<ws>` and keep its output: ignored files a checkout generates are expected. Tracked changes: recreate the workspace at the base branch's tip.
2. Create an empty marker file outside every repo, e.g. `<workspaces folder>/.scratch/planner-<team>-<run>.mark`. Its modification time is the line the run must not cross.

**After the run**

3. List everything in `<ws>` changed after the marker, outside `.git` and `.jj`. Directories count: adding or deleting a file changes its directory's time, so this catches deletions and ignored files too.
   - bash or zsh: `find <ws> -cnewer <mark> -not -path '*/.git/*' -not -path '*/.jj/*' -not -name .git -not -name .jj` (`-cnewer`: a write can restore a file's modified time, not its change time)
   - PowerShell: `$t=(Get-Item <mark>).LastWriteTime; @(Get-Item <ws>) + @(Get-ChildItem <ws> -Recurse -Force) | Where-Object { $_.LastWriteTime -gt $t -and $_.FullName -notmatch '[\\/]\.(git|jj)([\\/]|$)' }`
4. Run step 1's status command again and compare it with the kept output.

**On any change** (step 3 lists anything, or step 4 differs): stop the slice, show the Engineer what changed (the list, and `git diff` or `jj diff`), don't save the plan, and recreate the workspace before the next run; in jj, first `jj abandon` the workspace's working-copy commit, which already snapshotted the write. Writes outside `<ws>` are not detected.
