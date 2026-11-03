# handoff

How Claude hands the Engineer something to do. `/handoff:how-to` turns every ask (run this, click that, review this) into a very short worksheet: numbered steps, one action each, exact commands or click paths, a "Done when" per step, and a final "Tell me" line. A fork is settled with a question first; a worksheet never contains options.

Claude applies it on its own whenever it asks the Engineer to act. No other plugin needs it; `zaf setup` prints its own worksheet for whatever is left for the Engineer.
