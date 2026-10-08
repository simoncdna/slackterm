# One screenshot per theme, assembled into docs/media/themes.png by
# docs/record.sh. The animation itself is thrown away.

Output target/vhs/themes.gif
Source docs/tapes/common.tape

Type "slackterm demo"
Enter
Sleep 1.5s
Ctrl+K
Type "release"
Enter
Sleep 600ms
Type "kkkk"
Type "t"
Sleep 800ms
Show

# Screenshots land a frame late: wait before and after each one.
Sleep 800ms
Screenshot target/vhs/theme-1.png
Sleep 500ms
Hide
Type ","
Type "j"
Type "l"
Escape
Show
Sleep 800ms
Screenshot target/vhs/theme-2.png
Sleep 500ms
Hide
Type ","
Type "j"
Type "l"
Escape
Show
Sleep 800ms
Screenshot target/vhs/theme-3.png
Sleep 500ms
Hide
Type ","
Type "j"
Type "l"
Escape
Show
Sleep 800ms
Screenshot target/vhs/theme-4.png
Sleep 500ms
