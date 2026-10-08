# Sidebar sections that fold, the private tab and the fuzzy switcher.

Output docs/media/navigation.gif
Source docs/tapes/common.tape

Type "slackterm demo"
Enter
Sleep 1.5s
Show
Sleep 1s

# Fold the "Lancement v2" section: unread conversations stay visible.
Type@220ms "kkkkkkkk"
Sleep 600ms
Enter
Sleep 1.8s
Enter
Sleep 1.2s

# The private tab lists direct messages by recency.
Type "l"
Sleep 1.8s
Type@400ms "jjk"
Sleep 500ms
Enter
Sleep 2s

# Back to the channels, then fuzzy-jump to #design.
Type "h"
Sleep 400ms
Type "h"
Sleep 1s
Ctrl+K
Sleep 700ms
Type@180ms "dsg"
Sleep 900ms
Enter
Sleep 2.5s
