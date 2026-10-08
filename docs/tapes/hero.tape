# The tour shown at the top of the README: sidebar, channel, thread reply,
# switcher and a direct message, while messages arrive in real time.

Output docs/media/hero.gif
Source docs/tapes/common.tape

Show
Sleep 600ms
Type "slackterm demo"
Sleep 500ms
Enter
Sleep 2.2s

# Up the sidebar to #release-2-0, where someone mentioned us.
Type@260ms "kkkkk"
Sleep 600ms
Enter
Sleep 2s

# Reply in a thread to the message that mentions us.
Type "k"
Sleep 900ms
Type "t"
Sleep 1.4s
Type "i"
Sleep 300ms
Type@45ms "Oui ! Je les envoie demain matin :memo:"
Sleep 500ms
Enter
Sleep 2.6s
Escape
Sleep 300ms
Escape
Sleep 900ms

# Jump to a direct message with Ctrl+K and get an answer.
Ctrl+K
Sleep 600ms
Type@140ms "cam"
Sleep 700ms
Enter
Sleep 1.2s
Type "i"
Sleep 300ms
Type@45ms "Je regarde le déroulé tout de suite :eyes:"
Sleep 400ms
Enter
Sleep 3.6s
Escape
Sleep 1.5s
