# Message search: results with highlighted terms, opening a thread reply,
# then the in: and from: filters.

Output docs/media/search.gif
Source docs/tapes/common.tape

Type "slackterm demo"
Enter
Sleep 1.5s
Show
Sleep 1s

Type "/"
Sleep 700ms
Type@90ms "migration"
Sleep 400ms
Enter
Sleep 2.2s
Down
Sleep 700ms
Up
Sleep 700ms
Enter
Sleep 3s

Type "/"
Sleep 600ms
Type@70ms "from:@hugo in:#dev"
Sleep 400ms
Enter
Sleep 2.2s
Down
Sleep 600ms
Enter
Sleep 2.5s
