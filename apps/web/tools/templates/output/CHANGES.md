# 내장 템플릿: 변환이 바꾼 곳

`node tools/templates/convert.ts --write tools/templates/output`이 쓴 파일이다. 손으로 고치지 않는다.

원본의 내장 템플릿 13개를 새 형식으로 바꾸면서 글에서 바꾼 곳을 모두 적는다. 규칙은 다섯이다. 글을 고쳐 쓰지는 않았다(검수는 나중에 사람이 한다). 이전 칸의 `{old product name}`, `{old company name}`은 원본의 두 이름이 있던 자리다(이 폴더에는 그 이름을 적지 않는다).

| 규칙 | 건수 |
| --- | --- |
| 원본 이름 → fvoci | 9 |
| 제품을 가리키는 "Boards" → fvoci | 5 |
| 바깥 링크와 그 문장을 뺌 | 1 |
| 원본 회사로 가는 링크 속성 값을 뺌 | 22 |
| 그 밖의 바깥 주소인 링크 속성 값을 example.com 아래로 옮김 | 10 |
| 합계 | 47 |

- **제품을 가리키는 "Boards"**는 결정문의 글자 그대로("원본 이름을 바꾼다")보다 한 걸음 나간 것이다. 번역에서 같은 낱말을 제품 이름일 때 fvoci로 바꾸기로 한 결정을 따랐다. 다섯 곳을 읽고 모두 제품을 가리킨다고 판단했다. 그대로 두려면 `rewrite.ts`의 규칙 하나를 끄면 된다.
- 기계적으로 바꾼 결과가 어색한 곳이 있다(예: `fvoci plugin`, 한 문장에 fvoci가 두 번). 고쳐 쓰지 않고 그대로 두었다.
- **링크 속성의 값**은 도메인으로 갈랐다. 호스트의 한 조각이 원본의 두 이름 가운데 하나이면 원본 회사로 가는 주소로 보고 값을 뺐다. 그 밖의 바깥 주소(지어낸 회사의 사이트, 다른 사이트의 글·그림·게시물)는 `example.com` 아래로 옮겼다: 호스트에서 `www.`와 마지막 조각을 떼고 `.example.com`을 붙이고, 스킴은 `https`로, 경로·질의·조각은 그대로 둔다(그 안의 원본 이름은 fvoci로). 예시 데이터가 링크처럼 보이되 실제 사이트로는 가지 않는다. 규칙은 `rewrite.ts`의 `rewriteLinkValue` 하나다.

## Meeting Agenda (`01-meeting-agenda.json`)

바뀐 곳이 없다.

## Sales Pipeline CRM (`02-sales-pipeline-crm.json`)

바뀐 곳이 없다.

## Personal Tasks (`03-personal-tasks.json`)

바뀐 곳이 없다.

## Project Tasks (`04-project-tasks.json`)

바뀐 곳이 없다.

## Company Goals & OKRs (`05-company-goals-okrs.json`)

바뀐 곳이 없다.

## Personal Goals (`06-personal-goals.json`)

바뀐 곳이 없다.

## Sprint Planner (`07-sprint-planner.json`)

| 어디 | 무엇 | 규칙 | 이전 | 이후 |
| --- | --- | --- | --- | --- |
| card: Horizontal scroll issue | body | 원본 이름 → fvoci | Edition: [e.g. Personal Desktop / Personal Server / {old company name} plugin] | Edition: [e.g. Personal Desktop / Personal Server / fvoci plugin] |
| card: Login screen not loading | body | 원본 이름 → fvoci | Edition: [e.g. Personal Desktop / Personal Server / {old company name} plugin] | Edition: [e.g. Personal Desktop / Personal Server / fvoci plugin] |
| card: Move cards across boards | value of "Design Link" | 원본 회사로 가는 링크 속성 값을 뺌 | https://{old company name}.com/boards/ | (없음) |
| card: Cross-team collaboration | value of "Design Link" | 원본 회사로 가는 링크 속성 값을 뺌 | https://{old company name}.com/boards/ | (없음) |
| card: Bug | body | 원본 이름 → fvoci | Edition: [e.g. Personal Desktop / Personal Server / {old company name} plugin] | Edition: [e.g. Personal Desktop / Personal Server / fvoci plugin] |
| card: Standard properties | value of "Design Link" | 원본 회사로 가는 링크 속성 값을 뺌 | https://{old company name}.com/boards/ | (없음) |
| card: Epic | value of "Design Link" | 원본 회사로 가는 링크 속성 값을 뺌 | https://{old company name}.com/boards/ | (없음) |
| card: Global templates | value of "Design Link" | 원본 회사로 가는 링크 속성 값을 뺌 | https://{old company name}.com/boards/ | (없음) |

## User Research Sessions (`08-user-research-sessions.json`)

| 어디 | 무엇 | 규칙 | 이전 | 이후 |
| --- | --- | --- | --- | --- |
| card: Frank Nash | value of "Recording URL" | 그 밖의 바깥 주소인 링크 속성 값을 example.com 아래로 옮김 | https://user-images.githubusercontent.com/46905241/121941290-ee355280-cd03-11eb-9b9f-f6f524e4103e.gif | https://user-images.githubusercontent.example.com/46905241/121941290-ee355280-cd03-11eb-9b9f-f6f524e4103e.gif |
| card: Richard Parsons | value of "Recording URL" | 그 밖의 바깥 주소인 링크 속성 값을 example.com 아래로 옮김 | https://user-images.githubusercontent.com/46905241/121941290-ee355280-cd03-11eb-9b9f-f6f524e4103e.gif | https://user-images.githubusercontent.example.com/46905241/121941290-ee355280-cd03-11eb-9b9f-f6f524e4103e.gif |
| card: Claire Hart | value of "Recording URL" | 그 밖의 바깥 주소인 링크 속성 값을 example.com 아래로 옮김 | https://user-images.githubusercontent.com/46905241/121941290-ee355280-cd03-11eb-9b9f-f6f524e4103e.gif | https://user-images.githubusercontent.example.com/46905241/121941290-ee355280-cd03-11eb-9b9f-f6f524e4103e.gif |
| card: Olivia Alsop | value of "Recording URL" | 그 밖의 바깥 주소인 링크 속성 값을 example.com 아래로 옮김 | https://user-images.githubusercontent.com/46905241/121941290-ee355280-cd03-11eb-9b9f-f6f524e4103e.gif | https://user-images.githubusercontent.example.com/46905241/121941290-ee355280-cd03-11eb-9b9f-f6f524e4103e.gif |

## Competitive Analysis (`09-competitive-analysis.json`)

| 어디 | 무엇 | 규칙 | 이전 | 이후 |
| --- | --- | --- | --- | --- |
| card: Liminary Corp. | value of "Website" | 그 밖의 바깥 주소인 링크 속성 값을 example.com 아래로 옮김 | liminarycorp.com | https://liminarycorp.example.com |
| card: Helx Industries | value of "Website" | 그 밖의 바깥 주소인 링크 속성 값을 example.com 아래로 옮김 | helxindustries.com | https://helxindustries.example.com |
| card: Kadera Global | value of "Website" | 그 밖의 바깥 주소인 링크 속성 값을 example.com 아래로 옮김 | kaderaglobal.com | https://kaderaglobal.example.com |
| card: Ositions Inc. | value of "Website" | 그 밖의 바깥 주소인 링크 속성 값을 example.com 아래로 옮김 | ositionsinc.com | https://ositionsinc.example.com |
| card: Afformance Ltd. | value of "Website" | 그 밖의 바깥 주소인 링크 속성 값을 example.com 아래로 옮김 | afformanceltd.com | https://afformanceltd.example.com |

## Content Calendar (`10-content-calendar.json`)

| 어디 | 무엇 | 규칙 | 이전 | 이후 |
| --- | --- | --- | --- | --- |
| card: New Project and Workflow Management Solutions for Developers | value of "Link" | 원본 회사로 가는 링크 속성 값을 뺌 | https://{old company name}.com/newsroom/press-releases/{old company name}-launches-new-project-and-workflow-management-solutions-for-developers/ | (없음) |
| card: [Tweet] fvoci v6.1 includes card @-mention notifications in fvoci | title | 원본 이름 → fvoci | [Tweet] {old company name} v6.1 includes card @-mention notifications in Boards | [Tweet] fvoci v6.1 includes card @-mention notifications in fvoci |
| card: [Tweet] fvoci v6.1 includes card @-mention notifications in fvoci | title | 제품을 가리키는 "Boards" → fvoci | [Tweet] {old company name} v6.1 includes card @-mention notifications in Boards | [Tweet] fvoci v6.1 includes card @-mention notifications in fvoci |
| card: [Tweet] fvoci v6.1 includes card @-mention notifications in fvoci | value of "Link" | 원본 회사로 가는 링크 속성 값을 뺌 | https://twitter.com/{old company name}/status/1463145633162969097?s=20 | (없음) |
| card: Top 10 Must-Have DevOps Tools in 2021 | value of "Link" | 그 밖의 바깥 주소인 링크 속성 값을 example.com 아래로 옮김 | https://www.toolbox.com/tech/devops/articles/best-devops-tools/ | https://toolbox.example.com/tech/devops/articles/best-devops-tools/ |
| card: Unblocking Workflows: The Guide to Developer Productivity | value of "Link" | 원본 회사로 가는 링크 속성 값을 뺌 | https://{old company name}.com/newsroom/press-releases/{old company name}-unveils-definitive-report-on-the-state-of-developer-productivity-unblocking-workflows-the-guide-to-developer-productivity-2022-edition/ | (없음) |

## Team Retrospective (`11-team-retrospective.json`)

바뀐 곳이 없다.

## Roadmap (`12-roadmap.json`)

| 어디 | 무엇 | 규칙 | 이전 | 이후 |
| --- | --- | --- | --- | --- |
| card: App crashing | body | 원본 이름 → fvoci | Edition: [e.g. Personal Desktop / Personal Server / {old company name} plugin] | Edition: [e.g. Personal Desktop / Personal Server / fvoci plugin] |
| card: Calendar view | value of "Design Link" | 원본 회사로 가는 링크 속성 값을 뺌 | https://{old company name}.com/boards/ | (없음) |
| card: Standard templates | value of "Design Link" | 원본 회사로 가는 링크 속성 값을 뺌 | https://{old company name}.com/boards/ | (없음) |
| card: Import / Export | value of "Design Link" | 원본 회사로 가는 링크 속성 값을 뺌 | https://{old company name}.com/boards/ | (없음) |
| card: Review API design | value of "Design Link" | 원본 회사로 가는 링크 속성 값을 뺌 | https://{old company name}.com/boards/ | (없음) |
| card: Icons don't display | value of "Design Link" | 원본 회사로 가는 링크 속성 값을 뺌 | https://{old company name}.com/boards/ | (없음) |
| card: Icons don't display | body | 원본 이름 → fvoci | Edition: [e.g. Personal Desktop / Personal Server / {old company name} plugin] | Edition: [e.g. Personal Desktop / Personal Server / fvoci plugin] |

## Welcome to fvoci! (`13-welcome.json`)

| 어디 | 무엇 | 규칙 | 이전 | 이후 |
| --- | --- | --- | --- | --- |
| board: Welcome to fvoci! | title | 제품을 가리키는 "Boards" → fvoci | Welcome to Boards! | Welcome to fvoci! |
| board: Welcome to fvoci! | description | 원본 이름 → fvoci | {old company name} Boards is an open source project management tool that helps you organize, track, and manage work across teams. Select a card to learn more. | fvoci is an open source project management tool that helps you organize, track, and manage work across teams. Select a card to learn more. |
| card: Drag cards | value of "Reference" | 원본 회사로 가는 링크 속성 값을 뺌 | https://docs.{old company name}.com/boards/working-with-boards.html#dragging-cards | (없음) |
| card: Drag cards | body | 원본 이름 → fvoci | {old company name} Boards makes it easy for you to update certain properties on cards through our drag and drop functionality. Simply drag this card from the Later column to the Completed column to automatically update the status and mark this task as complete. | fvoci makes it easy for you to update certain properties on cards through our drag and drop functionality. Simply drag this card from the Later column to the Completed column to automatically update the status and mark this task as complete. |
| card: Manage tasks with cards | value of "Reference" | 원본 회사로 가는 링크 속성 값을 뺌 | https://docs.{old company name}.com/boards/work-with-cards.html | (없음) |
| card: Create your own board | value of "Reference" | 원본 회사로 가는 링크 속성 값을 뺌 | https://docs.{old company name}.com/boards/working-with-boards.html#adding-new-boards | (없음) |
| card: Share a board | value of "Reference" | 원본 회사로 가는 링크 속성 값을 뺌 | https://docs.{old company name}.com/boards/sharing-boards.html | (없음) |
| card: Create a new card | value of "Reference" | 원본 회사로 가는 링크 속성 값을 뺌 | https://docs.{old company name}.com/boards/working-with-boards.html#adding-cards | (없음) |
| card: Create a new card | body | 원본 이름 → fvoci | {old company name} Boards helps you manage and track all your project tasks with Cards. | fvoci helps you manage and track all your project tasks with Cards. |
| card: Share cards on Channels | value of "Reference" | 원본 회사로 가는 링크 속성 값을 뺌 | https://docs.{old company name}.com/boards/work-with-cards.html#share-card-previews | (없음) |
| card: Share cards on Channels | body | 제품을 가리키는 "Boards" → fvoci | Cards can be linked and shared with teammates directly on Channels. Card previews are displayed when shared on Channels, so your team can discuss work items and get the relevant context without having to switch over to Boards. | Cards can be linked and shared with teammates directly on Channels. Card previews are displayed when shared on Channels, so your team can discuss work items and get the relevant context without having to switch over to fvoci. |
| card: Share cards on Channels | body | 제품을 가리키는 "Boards" → fvoci | After you've copied the link, paste it into any channel or Direct Message to share the card. A preview of the card will display within the channel with a link back to the card on Boards. | After you've copied the link, paste it into any channel or Direct Message to share the card. A preview of the card will display within the channel with a link back to the card on fvoci. |
| card: Create a new view | value of "Reference" | 원본 회사로 가는 링크 속성 값을 뺌 | https://docs.{old company name}.com/boards/working-with-boards.html#adding-new-views | (없음) |
| card: Add new properties | value of "Reference" | 원본 회사로 가는 링크 속성 값을 뺌 | https://docs.{old company name}.com/boards/work-with-cards.html#add-and-manage-properties | (없음) |
| card: Add new properties | body | 제품을 가리키는 "Boards" → fvoci | Customize cards to fit your needs and track the information most important to you. Boards supports a wide range of fully customizable property types. For example, you can: | Customize cards to fit your needs and track the information most important to you. fvoci supports a wide range of fully customizable property types. For example, you can: |
| card: @mention teammates | value of "Reference" | 원본 회사로 가는 링크 속성 값을 뺌 | https://docs.{old company name}.com/boards/work-with-cards.html#mention-people | (없음) |
| card: @mention teammates | body | 바깥 링크와 그 문장을 뺌 | To mention a teammate use the @ symbol with their username in the comments or description section. They'll get a Direct Message notification via Channels and also be added as a [follower](https://docs.{old company name}.com/boards/work-with-cards.html#receive-updates) to the card.  | To mention a teammate use the @ symbol with their username in the comments or description section. |

## 바꾸지 않았지만 검수할 때 볼 것

규칙에 걸리지 않아 그대로 둔 것이다.

| 종류 | 건수 |
| --- | --- |
| 예시 이메일 주소(실제로 있는 도메인) | 10 |
| 그림 파일(원본 아카이브의 것. 원본 화면을 찍은 것이 섞여 있다) | 15 |
| 원본 회사의 다른 제품(채널, 다이렉트 메시지)을 말하는 글 | 3 |

| 템플릿 | 종류 | 어디 | 글 |
| --- | --- | --- | --- |
| sales-pipeline-crm | 예시 이메일 주소(실제로 있는 도메인) | Jonathan Frazier / value of "Email" | jonathan.frazier@email.com |
| sales-pipeline-crm | 예시 이메일 주소(실제로 있는 도메인) | Richard Guzman / value of "Email" | richard.guzman@email.com |
| sales-pipeline-crm | 예시 이메일 주소(실제로 있는 도메인) | Byron Cole / value of "Email" | byron.cole@email.com |
| sales-pipeline-crm | 예시 이메일 주소(실제로 있는 도메인) | Caitlyn Russel / value of "Email" | caitlyn.russel@email.com |
| sales-pipeline-crm | 예시 이메일 주소(실제로 있는 도메인) | Shelby Olson / value of "Email" | shelby.olson@email.com |
| personal-tasks | 그림 파일(원본 아카이브의 것. 원본 화면을 찍은 것이 섞여 있다) | Go for a walk / body | 76fwrj36hptg6dywka4k5mt3sph.png |
| sprint-planner | 그림 파일(원본 아카이브의 것. 원본 화면을 찍은 것이 섞여 있다) | Horizontal scroll issue / body | 7tmfu5iqju3n1mdfwi5gru89qmw.png |
| sprint-planner | 그림 파일(원본 아카이브의 것. 원본 화면을 찍은 것이 섞여 있다) | Login screen not loading / body | 7b9xk9boj3fbqfm3umeaaizp8qr.png |
| sprint-planner | 그림 파일(원본 아카이브의 것. 원본 화면을 찍은 것이 섞여 있다) | Bug / body | 7tmfu5iqju3n1mdfwi5gru89qmw.png |
| user-research-sessions | 예시 이메일 주소(실제로 있는 도메인) | Frank Nash / value of "Email" | frank.nash@email.com |
| user-research-sessions | 예시 이메일 주소(실제로 있는 도메인) | Richard Parsons / value of "Email" | richard.parsons@email.com |
| user-research-sessions | 예시 이메일 주소(실제로 있는 도메인) | Claire Hart / value of "Email" | claire.hart@email.com |
| user-research-sessions | 예시 이메일 주소(실제로 있는 도메인) | Olivia Alsop / value of "Email" | olivia.alsop@email.com |
| user-research-sessions | 예시 이메일 주소(실제로 있는 도메인) | Bernadette Powell / value of "Email" | bernadette.powell@email.com |
| content-calendar | 그림 파일(원본 아카이브의 것. 원본 화면을 찍은 것이 섞여 있다) | [Tweet] fvoci v6.1 includes card @-mention notifications in fvoci / body | 7y5kr8x8ybpnwdykjfuz57rggrh.png |
| roadmap | 그림 파일(원본 아카이브의 것. 원본 화면을 찍은 것이 섞여 있다) | App crashing / body | 77pe9r4ckbin438ph3f18bpatua.png |
| roadmap | 그림 파일(원본 아카이브의 것. 원본 화면을 찍은 것이 섞여 있다) | Icons don't display / body | 7pbp4qg415pbstc6enzeicnu3qh.png |
| welcome | 그림 파일(원본 아카이브의 것. 원본 화면을 찍은 것이 섞여 있다) | Create your own board / body | 74uia99m9btr8peydw7oexn37tw.gif |
| welcome | 그림 파일(원본 아카이브의 것. 원본 화면을 찍은 것이 섞여 있다) | Share a board / body | 7knxbyuiedtdafcgmropgkrtybr.gif |
| welcome | 그림 파일(원본 아카이브의 것. 원본 화면을 찍은 것이 섞여 있다) | Create a new card / body | 7iw4rxx7jj7bypmdotd9z469cyh.png |
| welcome | 원본 회사의 다른 제품(채널, 다이렉트 메시지)을 말하는 글 | Share cards on Channels / body | Cards can be linked and shared with teammates directly on Channels. Card previews are displayed when shared on Channels, so your team can discuss work items and get the relevant context without having to switch over to fvoci. |
| welcome | 원본 회사의 다른 제품(채널, 다이렉트 메시지)을 말하는 글 | Share cards on Channels / body | After you've copied the link, paste it into any channel or Direct Message to share the card. A preview of the card will display within the channel with a link back to the card on fvoci. |
| welcome | 그림 파일(원본 아카이브의 것. 원본 화면을 찍은 것이 섞여 있다) | Share cards on Channels / body | 7ek6wbpp19jfoujs1goh6kttbby.gif |
| welcome | 그림 파일(원본 아카이브의 것. 원본 화면을 찍은 것이 섞여 있다) | Filter and sort cards / body | 7dybb6t8fj3nrdft7nerhuf784y.png |
| welcome | 그림 파일(원본 아카이브의 것. 원본 화면을 찍은 것이 섞여 있다) | Create a new view / body | 78jws5m1myf8pufewzkaa6i11sc.gif |
| welcome | 그림 파일(원본 아카이브의 것. 원본 화면을 찍은 것이 섞여 있다) | Add new properties / body | 7d6hrtig3zt8f9cnbo1um5oxx3y.gif |
| welcome | 원본 회사의 다른 제품(채널, 다이렉트 메시지)을 말하는 글 | @mention teammates / body | Whenever any changes are made to the card, they'll automatically get notified on Channels. |
| welcome | 그림 파일(원본 아카이브의 것. 원본 화면을 찍은 것이 섞여 있다) | @mention teammates / body | 74nt9eqzea3ydjjpgjtsxcjgrxc.gif |
