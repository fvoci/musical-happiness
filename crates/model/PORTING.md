# fvoci-model 포팅 노트

원본: `server/model/*.go`, `server/utils/utils.go`, `server/services/auth/email.go`.
서버와 웹 클라이언트가 함께 쓰는 와이어 타입이다. I/O와 시계 읽기가 없고, 시각이 필요한 함수는 현재 시각(ms)을 인자로 받는다.

## API 매핑

| 원본 | Rust | 비고 |
|---|---|---|
| `utils.IDType`, `utils.NewID` | `IdKind`, `new_id` | 접두 1글자 + UUID v4의 z-base-32, 27자 |
| `model.Block` | `Block` | `WorkspaceID`(JSON 제외)와 `Limited`(cloud limits 전용)는 없음 |
| `model.BlockType`, `BlockTypeFromString`, `BlockType2IDType` | `BlockType`, `FromStr`, `BlockType::id_kind` | 모르는 문자열은 `BlockType::Other`에 그대로 보존 |
| `Block.IsValid` | `Block::validate` → `BlockError` | |
| `BlockPatch`, `BlockPatch.Patch` | `BlockPatch`, `BlockPatch::apply` | |
| `BlockPatchBatch` | `BlockPatchBatch` | |
| `StampModificationMetadata` | `stamp_modification_metadata(user_id, blocks, now)` | 감사 기록 인자는 없음 |
| `GenerateBlockIDs` | `generate_block_ids` | 제자리 수정 |
| `Board`, `BoardType`, `BoardRole` | `Board`, `BoardType`, `BoardRole` | 두 enum은 닫힌 집합 |
| `Board.IsValid`, `BoardPatch.IsValid` | `Board::validate` → `BoardError` | 타입·역할 검사는 디코딩 단계로 이동(아래 차이 1) |
| `BoardPatch.Patch` | `BoardPatch::apply` | |
| `BoardMember` | `BoardMember` | |
| `BoardSearchField`, `BoardSearchFieldFromString` | `BoardSearchField`, `FromStr` | |
| `BoardsAndBlocks`, `DeleteBoardsAndBlocks`, `PatchBoardsAndBlocks` 와 각 `IsValid` | 같은 이름, `validate` → `BoardsAndBlocksError` | |
| `NewDeleteBoardsAndBlocksFromBabs` | `From<&BoardsAndBlocks> for DeleteBoardsAndBlocks` | |
| `GenerateBoardsAndBlocksIDs` | `generate_boards_and_blocks_ids` | 소유권을 받아 새 값을 반환 |
| `Card`, `Card.Populate`, `Card.CheckValid` | `Card`, `Card::populate(now)`, `Card::validate` | |
| `Block2Card`, `Card2Block` | `Card::from_block(block, now)`, `From<Card> for Block` | |
| `CardPatch`, `Patch`, `CheckValid`, `CardPatch2BlockPatch` | `CardPatch`, `apply`, `validate`, `to_block_patch` | |
| `Category`, `Hydrate`, `IsValid` | `Category`, `hydrate(now)`, `validate` → `CategoryError` | |
| `CategoryBoards`, `CategoryBoardMetadata`, `BoardCategoryWebsocketData` | 같은 이름 | |
| `User`, `UserPreferencesPatch`, `Session` | 같은 이름 | `Session`은 와이어에 나가지 않아 serde 구현 없음 |
| `LoginRequest`, `LoginResponse`, `RegisterRequest`, `ChangePasswordRequest` | 같은 이름, `validate` → `AuthParamError` | |
| `auth.IsEmailValid` | `is_email_valid` | 정규식 대신 같은 규칙을 직접 검사 |
| `Team`, `Sharing`, `Subscription`, `Subscriber`, `ClientConfig`, `ArchiveHeader`, `ArchiveLine`, `ErrorResponse` | 같은 이름 | |
| 호스트 애플리케이션의 파일 정보 구조체 (`GetFileInfo`가 돌려주는 값) | `FileInfo` | 원본 SQL이 읽고 쓰는 멤버와 `model.NewFileInfo`가 채우는 멤버만: `id`, `user_id`, `create_at`, `update_at`, `delete_at`, `name`, `extension`, `size`, `mime_type`, `archived`. `path`는 직렬화하지 않는다 |
| `SingleUser`, `GlobalTeamID`, `SystemUserID` | `SINGLE_USER_ID`, `GLOBAL_TEAM_ID`, `SYSTEM_USER_ID` | |

옮기지 않은 것:

- `properties.go`(`ParsePropertySchema`, `ParseProperties`, `PropDef.GetValue`): 호출처가 구독 알림(`server/services/notify/notifysubscriptions`)뿐이고 그 기능은 범위 밖이다.
- `BoardMetadata`, `BoardMemberHistoryEntry`, `compliance.go`, `cloud.go`, `board_statistics.go`, `notification.go`, `services_api.go`, `permission.go`의 채널·시스템 권한: 플러그인 모드나 라이선스 전용 기능이다. 보드 권한은 서버 크레이트에 enum으로 둔다.
- `*FromJSON` 헬퍼: `serde_json::from_*`로 대체한다.
- `error.go`의 상태 코드 분류(`IsErrBadRequest` 등): HTTP 계층의 일이라 서버 크레이트로 간다. 이 크레이트는 영역별 오류 타입만 정의한다.
- `utils.StructToMap`, `Intersection`, `IsCloudLicense`: 플러그인 어댑터 전용이거나 호출처가 없다. `DedupeStringArr`, `GetBaseFilePath`, 시각 헬퍼는 쓰는 크레이트(store, server)에 둔다.
- `version.go`: 서버 크레이트에서 `/ping` 응답을 옮길 때 다룬다.

## 테스트 매핑

원본 테스트 하나(서브테스트 단위)가 Rust 테스트 하나에 대응한다.

| 원본 | Rust (`crates/model/tests/`) |
|---|---|
| `block_test.go` `TestGenerateBlockIDs` 8개 서브테스트 | `block_ids.rs` `generate_block_ids_*` 8개 (각 테스트의 문서 주석에 원본 서브테스트 이름) |
| `block_test.go` `TestStampModificationMetadata/base case` | `block_ids.rs` `stamp_modification_metadata_base_case` |
| `card_test.go` `TestBlock2Card/Good block`, `/Not a card` | `card.rs` `card_from_block_good_block`, `card_from_block_not_a_card` |
| `boards_and_blocks_test.go` `TestIsValidBoardsAndBlocks` 4개 | `boards_and_blocks.rs` `boards_and_blocks_validate_*` 4개 |
| `boards_and_blocks_test.go` `TestGenerateBoardsAndBlocksIDs` 2개 | `boards_and_blocks.rs` `generate_boards_and_blocks_ids_*` 2개 |
| `boards_and_blocks_test.go` `TestIsValidPatchBoardsAndBlocks` 4개 | `boards_and_blocks.rs` `patch_boards_and_blocks_validate_*` 4개 |
| `boards_and_blocks_test.go` `TestIsValidDeleteBoardsAndBlocks` | 원본에서 주석 처리된 테스트다("TODO fix this"). 그 안의 "no block ids" 케이스는 코드가 돌려주지 않는 오류를 기대한다. `delete_boards_and_blocks_validate_requires_only_boards`가 코드의 실제 동작을 고정한다 |
| `properties_test.go` (2개 함수) | 해당 없음: `properties.go`를 옮기지 않았다 |
| `server/services/auth/password_test.go`, `request_parser_test.go` | 서버 크레이트(S3)에서 옮긴다 |
| `webapp/src/blocks/*.test.ts` 19케이스 | 해당 없음: 전부 클라이언트 쪽 로직이다(`createPatchesFromBlocks`/`createPatchesFromBoards` 11, `FilterClause` 5, 뷰 정렬 3). 서버 모델에 대응하는 타입이 없어 웹앱 트랙의 `fvoci-viewmodel`로 간다 |

원본에 테스트가 없어 동작 목록에서 새로 쓴 특성화 테스트: `wire_format.rs`(JSON 멤버 이름, null·누락 처리, 패치 적용), `validation.rs`(블록·보드·인증 요청 검증, 이메일 규칙), `card.rs`의 변환·패치 테스트, `block_ids.rs`의 `cardOrder`·비정상 타입·single-user 케이스.

## 의도적 차이

1. **닫힌 집합은 디코딩에서 거절한다.** 원본은 `Board.type`, `minimumRole`, `Category.type`, `subscriberType`에 임의의 문자열을 받아 놓고 `IsValid`에서 400으로 거절한다(`invalid-board-type` 등). 포트는 enum이라 JSON 디코딩이 실패한다. 상태 코드는 같은 400이지만 오류 메시지 문구가 다르다. `Board.type`과 `subscriberType`은 필수 멤버가 됐다(원본은 누락을 빈 문자열로 읽고 `IsValid`에서 거절).
2. **맵·목록 멤버가 `null`이거나 없으면 빈 값으로 읽고, 빈 값으로 내보낸다.** 블록의 `fields`, 보드의 `properties`·`cardProperties` 등이 해당한다. 원본은 nil로 두어 응답에 `"fields": null`, `"properties": null`, `"cardProperties": null`이 나가고 DB에도 `null`로 저장되며, 그런 블록에 `updatedFields` 패치를 보내면 nil 맵 대입으로 패닉해 500이 된다(오라클에서 확인: `assignment to entry in nil map`). 포트는 `{}`와 `[]`를 내보내고 패치도 정상 적용한다. 원본 웹 클라이언트는 항상 객체와 배열을 보내므로 정상 입력에서는 차이가 없다.
3. **`generate_block_ids`는 타입이 맞지 않는 필드를 경고 로그 없이 그대로 둔다.** 원본은 경고를 남기고, 중첩 배열 안에 문자열이 아닌 값이 있으면 패닉한다.
4. **`generate_boards_and_blocks_ids`에서 식별자가 같은 보드가 둘이면** 포트는 첫 보드에 블록을 붙인다. 원본은 같은 블록을 두 번 나열하고 마지막 보드에 붙인다. 정상 입력이 아니다.
5. **`Block::validate`의 필드 크기는 `serde_json` 직렬화 기준이다.** 원본은 `<`, `>`, `&`를 `<`처럼 6글자로 이스케이프해서 세므로, 그런 문자가 많고 80만 자 한계에 걸친 입력에서만 판정이 갈린다.
6. **JSON 키의 대소문자를 구분한다.** 원본 디코더는 키를 대소문자 구분 없이 맞춘다(`"TITLE"`도 `title`로 읽음). 원본 클라이언트는 정확한 이름만 보낸다.
7. **카드 아이콘의 글자 수는 `unicode-segmentation`의 확장 그래핌 클러스터로 센다.** 원본은 `rivo/uniseg`를 쓴다. 유니코드 버전 차이로 최신 이모지 시퀀스에서 갈릴 수 있다.
8. **이름 치환:** 원본의 상수 `PreferencesCategory…`에 해당하는 값은 이 크레이트에 없다(스토어 크레이트 참고).

## 보존한 quirk

- **`BoardPatch::apply`의 카드 속성 병합:** 같은 패치가 한 속성을 삭제하면서 갱신하면 그 속성이 결과에 두 번 나온다(원래 자리와 끝). 보드에 `id`가 문자열이 아닌 속성 정의가 있으면 카드 속성을 건드리는 패치에서 조용히 사라진다. (`server/model/board.go:226-270`)
- **`CategoryBoards`의 JSON은 항상 `"sortOrder": 0`이다.** 원본에서 바깥 구조체의 대입되지 않는 `SortOrder`가 포함된 `Category`의 것을 가린다(`server/model/category_boards.go:8-16`). 클라이언트는 응답 배열의 순서로 정렬한다.
- **`CardPatch::to_block_patch`는 `properties` 필드 전체를 갱신된 속성으로 바꾼다.** 블록에 적용하면 카드의 다른 속성 값이 사라진다(`server/model/card.go:306-308`). `CardPatch::apply`는 병합한다.
- **`Card::from_block`은 중첩 배열이 든 `contentOrder`를 거절한다.** 웹 앱이 나란히 놓인 콘텐츠에 쓰는 형태다(`server/model/card.go:226-239`).
- **`generate_block_ids`:** `contentOrder`가 배열이 아닌 블록은 그 블록의 `defaultTemplateId`가 참조로 수집되지 않는다(`server/model/blockid.go:33-41`의 `continue`).
- **`DeleteBoardsAndBlocks::validate`는 보드만 요구한다.** 블록 목록은 비어도 된다.
- **비밀번호 최소 길이 8은 바이트 수다**(`server/model/auth.go:128`).
- **오류 메시지 문구는 원본 그대로다**(`"Board IDs do not match"`, `"invalid card, ID is missing"`, `"category team id ID cannot be empty"` 등). 응답의 `error` 멤버로 나가므로 Rust 관례(소문자, 마침표 없음)에 맞추지 않았다.

## 검증

2026-11-10, rustc 1.99.1:

- `cargo fmt --check`: 통과
- `cargo clippy -p fvoci-model --all-targets --all-features -- -D warnings`: 통과
- `cargo test -p fvoci-model`: 68개 통과 (단위 4, `block_ids` 12, `boards_and_blocks` 12, `card` 11, `validation` 9, `wire_format` 20)
- `RUSTDOCFLAGS="-D warnings" cargo doc -p fvoci-model --no-deps`: 통과
- 원본 Go 테스트는 이 단계에서 따로 돌리지 않았다. JSON 형식이 오라클과 일치하는지는 서버 트랙 S5의 차등 비교에서 확인한다.
