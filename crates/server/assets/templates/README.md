# 내장 템플릿

내장 템플릿 13개가 사는 단 하나의 자리다. 서버가 이 폴더의 `NN-<키>.json`을 바이너리에 넣고(`src/app/templates.rs`의 `TEMPLATE_FILES`), 웹앱의 메모리 구현도 같은 파일을 읽는다(`apps/web/src/lib/state/memory/seed.ts`). 원본의 템플릿 아카이브에서 한 번 변환해 만들었고, 변환기는 지웠다.

- **글은 이 폴더의 템플릿 파일에서 손으로 고친다.** 서버의 템플릿을 고친 판으로 다시 넣게 하려면 `src/app/templates.rs`의 `DEFAULT_TEMPLATE_VERSION`을 올린다.
- 파일을 더하거나 빼면 `TEMPLATE_FILES`도 고친다. 서버의 테스트(`tests/api/templates.rs`)가 이 폴더의 파일과 서버가 내는 템플릿을 견준다.
- 웹앱의 테스트(`apps/web/tests/tools/builtin-templates.test.ts`)가 본문이 편집기의 검증을 지나는지, 원본의 이름과 바깥 주소가 없는지, 날짜 값의 모양과 템플릿 안의 참조를 본다.
- `CHANGES.md`는 변환이 글에서 바꾼 47곳과 검수할 때 볼 것의 목록이다(변환했을 때의 기록).

## 템플릿 파일의 모양

모양은 서버의 `TemplateFile`(`src/app/templates.rs`)이 정하고, 모르는 멤버가 있으면 서버가 템플릿을 넣지 못한다.

```jsonc
{
  "key": "meeting-agenda",        // 제목이 바뀌어도 변하지 않는 이름
  "onboarding": false,            // 새 사용자가 받는 보드 하나만 true
  "board": { "id", "title", "description", "icon", "showDescription", "cardProperties": [{ "id", "name", "type", "options": [{ "id", "value", "color" }] }] },
  "views": [{ "id", "type": "view", "title", "fields": { /* viewType, groupById, cardOrder, filter, sortOptions, … 원본의 뷰 멤버 그대로 */ } }],
  "cards": [{ "id", "type": "card", "title", "fields": { "icon", "isTemplate", "properties": { "<속성 id>": "…" } }, "body": { "type": "doc", "content": [] } }],
  "files": [{ "fileId", "archiveMember", "bytes" }]
}
```

- **본문**(`body`)은 카드마다 문서 하나다(`body_version` 1의 허용 목록을 통과한다). 본문이 없는 카드는 `null`이다. 콘텐츠 블록과 `contentOrder`는 없다.
- **날짜 속성의 값**은 새 와이어 모양이다: `{"from": "2022-11-22"}`. 아카이브의 값은 모두 시각 없는 날짜였다(19건).
- **식별자**는 아카이브의 것 그대로다. 한 템플릿 안에서 뷰의 `cardOrder`, 카드의 선택지 값, 뷰의 `groupById` 같은 참조를 잇는 데만 쓴다. 서버가 넣을 때 새 식별자로 바꾼다(형식도 UUID가 아니다).
- **`onboarding`**: 원본은 온보딩 보드를 제목(`Welcome to …!`)으로 찾았다. 그 제목은 변환이 바꾼 글의 하나라서 표식을 따로 두었다. 서버는 제목이 아니라 이 표식으로 찾는다.
- **보드에 넣지 않은 것**: `isTemplate`·`templateVersion`·보드 종류(공개 여부)·`trackingTemplateId`·만든 사람과 시각. 서버가 넣을 때 정한다(원본은 내장 템플릿을 `isTemplate: true`, 공개 보드로 넣었다).
- **카드의 `isTemplate`**은 보드 안의 카드 템플릿이다(5장). 뷰의 `defaultTemplateId`가 가리킨다.
- **그림**(`files`): 늘 빈 목록이다. 원본의 본문에는 파일 14개를 가리키는 `image` 노드 15개가 있었는데, 서버 설계의 결정 18(3.10의 추가 안 B, 2026-11-11 04:06 KST 사용자 결정)대로 변환이 모두 뺐다. 그림만 담고 있던 노드도 함께 빠졌다. 멤버는 모양을 바꾸지 않으려고 남겼다.
