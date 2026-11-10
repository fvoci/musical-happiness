# 조각: 본문 허용 목록의 변경 (`body_version` 2) — 서버 세션에 넘길 것

작성 2026-11-10 23:52 KST. 웹앱 트랙의 편집기 작업(수식, 다이어그램, 이모지)에서 나온 것 가운데 서버의 검증·저장·계약이 맞춰야 하는 것만 적는다. 서버 설계 `docs/s6-design.md` 5.7절의 허용 목록(`body_version` 1)을 기준으로 **달라진 것만** 적는다. 웹앱 쪽의 구현은 `src/lib/editor/schema.ts`이고, 표 전체는 `docs/spike-editor.md` 2절에 있다.

## 1. 한눈에

| 항목                | 내용                                                                                                                        |
| ------------------- | --------------------------------------------------------------------------------------------------------------------------- |
| `body_version`      | **1 → 2**                                                                                                                   |
| 새 노드             | `inlineMath`(인라인, 원자), `blockMath`(블록, 원자). 둘 다 속성은 `latex` 하나                                              |
| 바뀐 노드·마크·한도 | 없다. 깊이 32, 노드 20,000개, 직렬화 1 MiB 그대로. 새 노드도 노드 하나로 센다                                               |
| 다이어그램          | **허용 목록을 바꾸지 않는다.** 언어가 `mermaid`인 `codeBlock`이다. 서버는 언어를 보지 않는다(지금의 `language` 규칙 그대로) |
| 이모지              | 바뀐 것 없다. 글자(`text`)다                                                                                                |
| 판 1의 문서         | 그대로 판 2의 문서다(새 노드를 더했을 뿐 뺀 것이 없다). 옮길 것이 없다                                                      |

## 2. 새 노드

| 노드         | 속성                                 | 안에 올 수 있는 것   | 어디에 올 수 있는가                                                                         |
| ------------ | ------------------------------------ | -------------------- | ------------------------------------------------------------------------------------------- |
| `inlineMath` | `latex`: 문자열, **1–4,000자, 필수** | 없음(`content` 없음) | 인라인 자리: `paragraph`, `heading` 안. `text`, `hardBreak`, `mention`과 같은 자리          |
| `blockMath`  | `latex`: 문자열, **1–4,000자, 필수** | 없음(`content` 없음) | 블록 자리: `doc`, `listItem`, `taskItem`, `blockquote` 안. `paragraph`, `image`와 같은 자리 |

- 블록 목록: `paragraph`, `heading`, `bulletList`, `orderedList`, `taskList`, `blockquote`, `codeBlock`, `horizontalRule`, `image`, **`blockMath`**.
- 인라인 목록: `text`, `hardBreak`, `mention`, **`inlineMath`**.
- `codeBlock` 안에는 여전히 `text`만 온다(`inlineMath`는 올 수 없다).
- `listItem`·`taskItem`의 첫 자식은 여전히 `paragraph`다(`blockMath`가 첫 자식일 수 없다).
- 다른 노드처럼 `marks`와 `text` 멤버를 가질 수 없다(`marks`는 `text` 노드에만).
- `latex` 밖의 속성은 거절한다. 그린 결과(HTML, MathML)는 저장하지 않는다. 저장하는 것은 **LaTeX 원문 문자열 하나**뿐이다.
- 이름은 편집기 라이브러리(Tiptap)의 공식 수식 확장이 쓰는 이름 그대로다(`inlineMath`, `blockMath`, 속성 `latex`).

예:

```json
{
  "type": "doc",
  "content": [
    {
      "type": "paragraph",
      "content": [
        { "type": "text", "text": "에너지는 " },
        { "type": "inlineMath", "attrs": { "latex": "E = mc^2" } },
        { "type": "text", "text": " 입니다." }
      ]
    },
    { "type": "blockMath", "attrs": { "latex": "\\int_0^1 x^2 \\, dx = \\frac{1}{3}" } },
    {
      "type": "codeBlock",
      "attrs": { "language": "mermaid" },
      "content": [{ "type": "text", "text": "flowchart LR\n  A --> B" }]
    }
  ]
}
```

### `latex`의 한도: 무엇을 세는가 (맞춰야 할 것)

웹앱의 검증기는 문자열 길이를 **UTF-16 코드 단위**로 센다(JavaScript의 `length`). 이것은 `latex`만이 아니라 판 1의 모든 길이 한도(`fileId` 100, `alt` 1000, `userId` 100, `href` 2048, `language` 40)에 똑같다. 5.7절은 "자"라고만 적었다. BMP 밖의 글자(이모지, 일부 한자)는 코드 단위 둘이므로, 서버가 유니코드 스칼라 값으로 세면 경계에서 두 구현이 어긋난다.

- **요청**: 서버도 UTF-16 코드 단위로 세거나, 다르게 센다면 알려 달라(웹앱이 맞춘다). 어느 쪽이든 공유 테스트 벡터에 아래 4절의 "BMP 밖의 글자" 경우를 넣어 고정한다.
- `latex`의 내용은 검증하지 않는다(문법, 명령어, 제어 문자 모두). 어떤 문자열이든 길이만 맞으면 받는다. 틀린 수식은 그리는 쪽이 원문과 안내로 보여 준다. 서버가 LaTeX를 해석할 일은 없다.

## 3. 서버가 뽑는 값

- **검색용 평문(`body_text`)**: 수식의 원문이 들어간다. `inlineMath`는 그 줄 안에 그 자리에 원문 그대로(앞뒤에 아무것도 붙이지 않는다), `blockMath`는 원문이 한 줄을 이룬다. 다이어그램은 `codeBlock`이므로 지금 규칙대로 원문이 들어간다(코드 블록의 글자가 한 줄, 안의 줄 바꿈은 그대로).
  - 웹앱의 `plainText`로 위 예를 뽑으면: `에너지는 E = mc^2 입니다.` / `\int_0^1 x^2 \, dx = \frac{1}{3}` / `flowchart LR⏎  A --> B` (줄마다 `\n`으로 잇는다. 빈 줄은 뺀다).
- **할 일 수**: 바뀐 것 없다(`taskItem`만 센다).
- **빈 본문 판정**: 바뀐 것 없다. 수식 하나만 있는 본문은 빈 본문이 아니다.
- **`file_refs`**: 바뀐 것 없다(수식과 다이어그램은 파일을 가리키지 않는다).

## 4. 공유 테스트 벡터(`contracts/body-vectors.json`)에 더할 경우

받는 예:

1. 문단 안의 `inlineMath`(글자 사이, 문단의 처음, 문단의 끝, 문단에 그것 하나뿐).
2. `heading` 안의 `inlineMath`.
3. `doc` 바로 아래의 `blockMath`. `blockquote` 안, `listItem`의 둘째 자식, `taskItem`의 둘째 자식.
4. `latex` 1자. `latex` 4,000자(ASCII).
5. `latex`가 공백뿐인 문자열(`" "`), 줄 바꿈이 든 문자열, `<script>`처럼 마크업으로 보이는 문자열, `\href{javascript:…}{x}`: **모두 받는다**(내용은 보지 않는다).
6. BMP 밖의 글자: `"😀"`를 2,000번 이은 `latex`(UTF-16으로 4,000) → 받는다.
7. 언어가 `mermaid`인 `codeBlock`(평범한 코드 블록과 같게 받는다). 내용이 `%%{init: …}%%`이든 `click A href "javascript:…"`이든 받는다.

거절하는 예:

8. `latex`가 없는 `inlineMath`·`blockMath`(`attrs` 없음, `attrs: {}`).
9. `latex`가 빈 문자열. `latex` 4,001자. `latex`가 문자열이 아님(숫자, `null`, 배열, 객체, 불리언).
10. BMP 밖의 글자: `"😀"` 2,000번 + `"x"`(UTF-16으로 4,001) → 거절. (스칼라 값으로 세면 2,001이라 받게 된다. 이 벡터가 2절의 물음을 고정한다.)
11. 모르는 속성: `{"latex":"x","display":true}`, `{"latex":"x","html":"<span>x</span>"}`.
12. 자리: `doc` 바로 아래의 `inlineMath`. `paragraph` 안의 `blockMath`. `codeBlock` 안의 `inlineMath`. `listItem`의 첫 자식인 `blockMath`.
13. `content`가 있는 수식. `marks`가 있는 수식. `text` 멤버가 있는 수식.

평문 벡터(서버가 `body_text`를 벡터로 고정한다면): 3절의 예 하나, `blockquote` 안의 `blockMath`, 수식만 있는 문단.

## 5. 서버가 하지 않아도 되는 것, 그리고 알아 둘 것

- **수식과 다이어그램은 보는 사람의 브라우저에서 그린다.** 서버는 원문을 저장하고 돌려줄 뿐이다. 그리는 쪽의 방어(KaTeX의 신뢰하지 않는 입력 설정, Mermaid의 엄격 설정과 그 뒤의 SVG 걸러 내기)는 `docs/spike-editor.md` 10절에 있다. 서버가 원문을 걸러 낼 필요는 없고, 걸러서도 안 된다(공격 문자열도 글자로는 정당한 본문이다).
- **콘텐츠 보안 정책을 둔다면**(지금 앱 페이지에는 없다):
  - `img-src 'self' data: blob:`와 `connect-src 'self'`를 권한다. 다이어그램 라이브러리는 그리는 동안 주소의 그림을 불러올 수 있는 문법이 있고(노드를 그림으로 그리기) 웹앱은 그 문법을 원문 단계에서 막지만, 정책이 한 겹 더 막아 준다.
  - `style-src`에 `'unsafe-inline'`이 없으면: 편집기와 수식은 동작한다(스크립트가 넣는 스타일시트를 쓰지 않고, 인라인 스타일은 CSSOM으로만 준다). **다이어그램은 색을 잃는다**: 그린 SVG 안의 `<style>` 요소가 막힌다. 그때는 웹앱이 그 스타일을 구성 가능한 스타일시트(`adoptedStyleSheets`)로 옮겨야 한다(`safe-svg.ts`의 한 곳). 정책을 정할 때 알려 달라.
  - `font-src 'self'`면 충분하다(KaTeX 글꼴은 앱의 `static/` 아래에서 온다).
- **정적 파일**: 빌드 산출물에 `KaTeX_*.woff2` 20개가 더해진다(해시가 든 이름, 이미 압축된 형식이라 `.br`·`.gz`를 만들지 않는다). 서버의 규칙(`has_content_hash`, `font/woff2`)은 그대로 맞는다.
- **고지**: 웹 고지(`static/THIRD_PARTY_NOTICES.txt`)에 KaTeX(MIT), KaTeX 글꼴(OFL-1.1), Mermaid와 그 의존성, CLDR 자료(Unicode-3.0)가 더해진다. 루트 `deny.toml`의 허용 목록은 손대지 않았다. `OFL-1.1`은 여전히 그 목록에 없고(글꼴에 한해 웹 도구가 받는다), ELK 레이아웃 엔진(`elkjs`, EPL-2.0 또는 GPL-3.0)은 번들에서 뺐다.
