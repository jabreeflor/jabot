# New Chat workspace picker

`new-chat.png` is a real screenshot of the Vite app at localhost:1420. The browser preview disables native folder and GitHub actions; those are available in the desktop host. The prompt field has been removed, and the chosen harness opens a blank session.

Coverage includes native-chooser cancellation, repository selection, clone errors/retry, sign-in routing, and real host session creation without an initial prompt. The desktop commands use the existing GitHub CLI authentication, paginated repository listing, and a bounded clone process.
