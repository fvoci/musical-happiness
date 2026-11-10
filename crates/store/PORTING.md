# fvoci-store 포팅 노트

원본: `server/services/store/store.go`, `server/services/store/sqlstore/*.go`, 테스트 `server/services/store/storetests/*.go`.
SQLite와 PostgreSQL을 지원하는 SQL 스토어다. 테이블·컬럼·부기(bookkeeping)는 원본 스키마 버전 40과 같아서, 한쪽 서버가 만든 DB를 다른 쪽이 열 수 있다.

## 설계

- **실행 계층 (`src/db.rs`)**: SQL 문장 하나를 두 DB가 함께 쓴다(`$n` 자리표시자). 값은 전부 바인딩하고, 테이블 접두사만 문자열로 들어간다(접속 시 식별자인지 검사). 방언 차이(JSON 캐스트, history 타임스탬프)는 `Sql`/`Scope`가 처리한다.
- **트랜잭션**: 문장이 둘 이상인 쓰기는 두 DB 모두에서 트랜잭션으로 실행한다. SQLite는 `BEGIN IMMEDIATE`로 연다.
- **스키마 (`schema/*.sql`, `src/schema.rs`)**: 빈 DB에는 버전 40 최종 스키마를 한 번에 만들고 원본과 같은 마이그레이션 기록(SQLite `db_migrations`, PostgreSQL `{prefix}schema_migrations`)과 데이터 마이그레이션 완료 키 3개를 쓴다. 버전 40이 아닌 DB는 거절한다.
- **SQLite 저널 모드**는 바꾸지 않는다(WAL을 켜지 않음). 인메모리 DB는 연결 하나로 쓴다.

## 스키마 대조 (2026-11-10)

오라클(원본 서버)이 빈 DB에 만든 스키마와 `schema/*.sql`을 적용한 결과를 비교했다.

- SQLite: 모든 테이블의 `PRAGMA table_info`, 인덱스 정의, `PRAGMA index_list`가 일치(차이 0줄).
- PostgreSQL 18.6: `information_schema.columns`, 제약 조건(이름 포함), `pg_indexes`가 일치. 유일한 차이는 `category_boards`의 `sort_order`/`hidden` 컬럼 서수(8, 9 대 7, 8)로, 원본이 마이그레이션 34에서 컬럼을 하나 지워 생긴 번호 공백이다.
- 원본이 PostgreSQL에 만드는 `db_lock` 테이블(마이그레이션 엔진의 잠금용)은 만들지 않는다. 원본은 필요할 때 스스로 만든다.

## 의도적 차이

1. **MySQL/MariaDB 미지원.** `dbtype: mysql`은 시작 오류다.
2. **버전 40 미만 스키마의 업그레이드 미지원.** 원본 서버로 먼저 올려야 한다.
3. **SQLite에서도 트랜잭션.** 원본은 SQLite에서 `@withTransaction` 메서드도 트랜잭션 없이 실행한다(`sqlstore/public_methods.go`). 포트는 실패 시 전부 되돌린다.
4. **history 행이 충돌하지 않는다.** history 테이블의 기본 키는 `(id, insert_at)`이고 SQLite의 기본 `insert_at`은 밀리초 해상도라, 원본은 같은 레코드를 1ms 안에 두 번 쓰면 기본 키 충돌로 실패한다(원본 테스트에 `time.Sleep(1ms)` 회피 코드가 있다). 포트는 SQLite에서 프로세스 안에서 단조 증가하는 밀리초를, PostgreSQL에서 `clock_timestamp()`를 `insert_at`에 직접 넣는다.
5. **이미지·첨부 블록 삭제가 동작한다.** 원본은 블록을 지울 때 `FileInfo`라는 테이블을 갱신하는데(`sqlstore/blocks.go:430`, `:987`), standalone에는 그 테이블이 없다. 오라클 확인: 이미지 블록, 또는 이미지를 가진 카드를 지우면 500 `no such table: FileInfo`가 나고 블록은 남는다(SQLite에서는 history에 삭제 행만 남는다). 포트는 접두사가 붙은 `file_info`의 `delete_at`을 갱신하고 삭제를 끝낸다.
6. **사용자 설정(preferences)의 카테고리 값이 다르다.** → 루트 `PORTING.md`의 결정 항목 1.
7. **`dbconfig`에서 모르는 파라미터는 시작 오류다.** 원본 드라이버는 조용히 무시하거나 드라이버별로 다르게 처리한다.
8. **인메모리 SQLite는 스토어마다 따로다.** 원본은 `file::memory:?cache=shared`를 여는 모든 연결이 한 DB를 본다.
9. **부분 결과와 오류를 함께 돌려주지 않는다.** 원본의 `GetBlocksByIDs`, `GetBoardsInTeamByIds`, `GetUsersList`는 일부만 찾았을 때 찾은 목록과 `ErrNotAllFound`를 함께 돌려준다. 포트는 오류만 돌려준다. 호출하는 쪽은 오류일 때 목록을 쓰지 않는다(`server/app/blocks.go:106`, `server/app/auth.go:72-75`).
10. **패닉 대신 오류.** 패치 개수가 ID 개수보다 적으면 원본은 인덱스 범위 초과로 패닉한다(`sqlstore/blocks.go:335`, `sqlstore/boards_and_blocks.go:79,87`). 포트는 `BlockIdsAndPatchesMismatch`/`BoardIdsAndPatchesMismatch`를 돌려준다. 전역 팀 템플릿을 `properties` 없이 넣을 때의 nil 맵 패닉(`sqlstore/board.go:302`)도 없다.
11. **닫힌 집합의 저장 값.** 행의 보드 타입·최소 역할·구독자 타입이 모르는 값이면 `StoreError::Corrupt`다. 원본은 문자열을 그대로 읽는다. 카테고리 `type`이 NULL이거나 비어 있으면 `custom`으로 읽는다(원본의 기본값과 같다).
12. **NULL은 빈 값으로 읽는다.** 원본은 NULL 컬럼을 문자열로 스캔하다 실패한다. 원본 스스로는 NULL을 쓰지 않는다.
13. **`get_system_setting`은 없는 키에 `None`을 돌려준다.** 원본은 빈 문자열이다.
14. **사용자 설정 패치가 원자적이고, 결과는 이름순이다.** 원본은 문장마다 따로 실행하고 결과 순서가 실행마다 다르다(맵 순회).
15. **호출처가 없는 인자를 받지 않는다.** standalone 스토어가 무시하는 `teamID`, `asGuestID`, `showEmail`, `showName`, `excludeBots`(`sqlstore/user.go:105,217,226`)와 `GetTeamsForUser`의 `userID`(`sqlstore/team.go:130`)는 시그니처에 없다. `CanSeeUser`는 standalone에서 항상 참이라 스토어에 두지 않았다.
16. **`per_page`가 0이면 전체를 돌려준다.** 원본도 `PerPage > 0`일 때만 제한하지만(`sqlstore/blocks.go:73-79`), 페이지가 0이 아니면 `LIMIT` 없는 `OFFSET`을 만들어 SQLite에서 문법 오류가 날 것으로 본다(미확인).
17. **재정렬 문장의 파라미터 수.** 재정렬이 ID마다 바인딩 파라미터를 2개 쓰므로 한 번에 약 1만 6천 개가 넘는 ID는 SQLite 한도에 걸린다. 원본에는 한도가 없다.

## 보안 수정

원본에서 재현해 확인한 뒤 고친 것만 적는다.

| 원본 위치 | 무엇이 가능했나 | 확인 방법 (오라클, 2026-11-10) | 고친 방식 | 테스트 |
|---|---|---|---|---|
| `sqlstore/category.go:221` | 카테고리 재정렬의 `CASE` 문에 요청의 카테고리 ID가 SQL 문자열로 그대로 이어 붙는다. ID는 카테고리 생성 요청에서 클라이언트가 정할 수 있다(2차 SQL 인젝션) | ID가 `q'q`인 카테고리를 만든 뒤 재정렬 요청 → 500, 서버 로그 `near "q": syntax error`. ID가 `a' THEN 777 WHEN 'zz`이면 문장이 오류 없이 다른 의미로 실행됨 | 모든 ID를 바인딩 파라미터로 전달 | `categories::reorder_categories_treats_identifiers_as_data` |
| `sqlstore/category_boards.go:144` | 카테고리 안 보드 재정렬에서 같은 패턴. 보드 ID는 URL 경로에서 오고 존재 여부를 확인하지 않는다 | 보드 ID `q'q`를 카테고리에 넣은 뒤(200 `ok`) 재정렬 요청 → 500, 서버 로그 `near "q": syntax error` | 모든 ID를 바인딩 파라미터로 전달 | `category_boards::reorder_category_boards_clears_the_sort_order_of_unlisted_boards` 등 |

스토어 밖(앱 계층)에서 고치는 항목(다른 사용자의 카테고리에 보드 넣기 등)은 `crates/server/PORTING.md`에 있다.

## API 매핑

모든 메서드는 `Store`의 `pub async fn`이고 `Result<_, StoreError>`를 돌려준다. Go의 `bool` 인자는 enum으로 바뀌었다(`DuplicateAs`, `PublicBoards`, `BoardVisibility`, `HistoryOrder`).

| 원본 (`store.Store`) | Rust |
|---|---|
| `GetBlocks`, `GetBlocksWithParentAndType`, `GetBlocksWithParent`, `GetBlocksWithType`, `GetBlocksForBoard` | `get_blocks(&BlockFilter)` — `BlockFilter::board(id).with_parent(..).with_type(..)` |
| `GetBlocksByIDs` | `get_blocks_by_ids(&[String])` |
| `GetSubTree2` | `get_subtree(board_id, block_id, &SubtreeQuery)` |
| `GetBlock` | `get_block(block_id)` |
| `InsertBlock`, `InsertBlocks` | `insert_block(&mut Block, user_id)`, `insert_blocks(&mut [Block], user_id)` |
| `PatchBlock`, `PatchBlocks` | `patch_block(block_id, &BlockPatch, user_id)`, `patch_blocks(&BlockPatchBatch, user_id)` |
| `DeleteBlock`, `UndeleteBlock` | `delete_block(block_id, modified_by)`, `undelete_block(block_id, modified_by)` |
| `GetBlockCountsByType` | `get_block_counts_by_type()` |
| `GetBlockHistory`, `GetBlockHistoryDescendants` | `get_block_history(block_id, &HistoryQuery)`, `get_block_history_descendants(board_id, &HistoryQuery)` |
| `DuplicateBlock` | `duplicate_block(board_id, block_id, user_id, DuplicateAs)` |
| `InsertBoard`, `InsertBoardWithAdmin` | `insert_board(&mut Board, user_id)`, `insert_board_with_admin(&mut Board, user_id) -> BoardMember` |
| `PatchBoard`, `GetBoard` | `patch_board(board_id, &BoardPatch, user_id) -> Board`, `get_board(board_id)` |
| `GetBoardsForUserAndTeam` | `get_boards_for_user_and_team(user_id, team_id, PublicBoards)` |
| `GetBoardsInTeamByIds` | `get_boards_in_team_by_ids(&[String], team_id)` |
| `DeleteBoard`, `UndeleteBoard` | `delete_board(board_id, user_id)`, `undelete_board(board_id, modified_by)` |
| `GetBoardCount`, `GetBoardHistory` | `get_board_count()`, `get_board_history(board_id, &HistoryQuery)` |
| `SaveMember`, `DeleteMember` | `save_member(&BoardMember)`, `delete_member(board_id, user_id)` |
| `GetMemberForBoard`, `GetMembersForBoard`, `GetMembersForUser` | `get_member_for_board(board_id, user_id)`, `get_members_for_board(board_id)`, `get_members_for_user(user_id)` |
| `GetBoardMemberHistory` | `get_board_member_history(board_id, user_id, Option<u32>) -> Vec<BoardMemberHistoryEntry>` |
| `SearchBoardsForUser`, `SearchBoardsForUserInTeam` | `search_boards_for_user(term, BoardSearchField, user_id, PublicBoards)`, `search_boards_for_user_in_team(team_id, term, user_id)` |
| `GetTemplateBoards`, `RemoveDefaultTemplates` | `get_template_boards(team_id, user_id)`, `remove_default_templates(&[Board])` |
| `CreateBoardsAndBlocks`, `CreateBoardsAndBlocksWithAdmin` | `create_boards_and_blocks(&mut BoardsAndBlocks, user_id)`, `create_boards_and_blocks_with_admin(..) -> Vec<BoardMember>` |
| `PatchBoardsAndBlocks`, `DeleteBoardsAndBlocks` | `patch_boards_and_blocks(&PatchBoardsAndBlocks, user_id) -> BoardsAndBlocks`, `delete_boards_and_blocks(&DeleteBoardsAndBlocks, user_id)` |
| `DuplicateBoard` | `duplicate_board(board_id, user_id, Option<&str> /* to_team */, DuplicateAs) -> (BoardsAndBlocks, Vec<BoardMember>)` |
| `GetRegisteredUserCount` | `get_registered_user_count()` |
| `GetUserByID`, `GetUserByEmail`, `GetUserByUsername` | `get_user_by_id`, `get_user_by_email`, `get_user_by_username` |
| `GetUsersList`, `GetUsersByTeam`, `SearchUsersByTeam` | `get_users_list(&[String])`, `get_users_by_team()`, `search_users_by_team(query)` |
| `CreateUser`, `UpdateUser` | `create_user(&mut User)`, `update_user(&mut User)` |
| `UpdateUserPassword`, `UpdateUserPasswordByID` | `update_user_password(username, hash)`, `update_user_password_by_id(user_id, hash)` |
| `PatchUserPreferences`, `GetUserPreferences` | `patch_user_preferences(user_id, &UserPreferencesPatch) -> Vec<Preference>`, `get_user_preferences(user_id)` |
| `GetActiveUserCount` | `get_active_user_count(updated_seconds_ago)` |
| `GetSession`, `CreateSession`, `RefreshSession`, `UpdateSession`, `DeleteSession`, `CleanUpSessions` | `get_session(token, expire_seconds)`, `create_session(&Session)`, `refresh_session(&Session)`, `update_session(&Session)`, `delete_session(id)`, `clean_up_sessions(expire_seconds)` |
| `GetSystemSetting`, `GetSystemSettings`, `SetSystemSetting` | `get_system_setting(key) -> Option<String>`, `get_system_settings()`, `set_system_setting(key, value)` |
| `UpsertTeamSignupToken`, `UpsertTeamSettings`, `GetTeam`, `GetTeamCount` | `upsert_team_signup_token(&Team)`, `upsert_team_settings(&Team)`, `get_team(id)`, `get_team_count()` |
| `GetTeamsForUser`, `GetAllTeams` | `get_all_teams()` (standalone에서는 둘이 같은 질의다) |
| `UpsertSharing`, `GetSharing` | `upsert_sharing(&Sharing)`, `get_sharing(board_id)` |
| `CreateSubscription`, `DeleteSubscription`, `GetSubscription`, `GetSubscriptions` | `create_subscription(Subscription)`, `delete_subscription(block_id, subscriber_id)`, `get_subscription(..)`, `get_subscriptions(subscriber_id)` |
| `GetSubscribersForBlock`, `GetSubscribersCountForBlock` | `get_subscribers_for_block(block_id)`, `get_subscribers_count_for_block(block_id)` |
| `GetFileInfo`, `SaveFileInfo` | `get_file_info(id) -> FileInfo`, `save_file_info(&FileInfo)` |
| `GetCategory`, `GetUserCategories`, `CreateCategory`, `UpdateCategory`, `DeleteCategory`, `ReorderCategories` | `get_category(id)`, `get_user_categories(user_id, team_id)`, `create_category(&Category)`, `update_category(&Category)`, `delete_category(id, user_id, team_id)`, `reorder_categories(user_id, team_id, Vec<String>)` |
| `GetUserCategoryBoards`, `AddUpdateCategoryBoard`, `ReorderCategoryBoards`, `SetBoardVisibility` | `get_user_category_boards(user_id, team_id)`, `add_update_category_board(user_id, category_id, &[String])`, `reorder_category_boards(category_id, Vec<String>)`, `set_board_visibility(user_id, category_id, board_id, BoardVisibility)` |
| `DBType`, `DBVersion`, `Shutdown` | `db_type()`, `db_version()`, `close()` |
| `sqlstore.New` + `server.NewStore` | `Store::connect(DbType, dbconfig, table_prefix)` |

옮기지 않은 메서드와 이유:

- standalone 서버 코드가 호출하지 않는다: `GetBlockHistoryNewestChildren`, `GetBoardAndCardByID`, `GetBoardAndCard`, `UpdateSubscribersNotifiedAt`, `CanSeeUser`(항상 참).
- 범위 밖 기능: 알림 힌트 4개(`*NotificationHint`), 컴플라이언스 3개, `RunDataRetention`, 카드 한도 3개(`GetUsedCardsCount` 등), 호스트 애플리케이션 전용(`GetLicense`, `SearchUserChannels`, `GetChannel`, `PostMessage`, `SendMessage`, `GetUserTimezone`).
- 테스트 전용: `DeleteBoardRecord`, `DeleteBlockRecord`.
- 마이그레이션 코드 전체(`migrate.go`, `data_migrations.go`, `schema_table_migration.go`, `legacy_blocks.go`): 의도적 차이 2.

## 테스트 매핑

원본 스위트(`storetests/*.go`)의 테스트 함수 하나가 Rust 테스트 하나다. 원본의 서브테스트들은 한 스토어를 공유하며 순서대로 실행되므로, Rust 테스트 안에 원본 서브테스트 이름을 주석으로 단 구간으로 순서대로 들어 있다. 위치는 `crates/store/tests/store/`.

| 원본 파일 | 원본 테스트 함수 → Rust 테스트 |
|---|---|
| `blocks.go` | `testInsertBlock` → `blocks::insert_block`, `testInsertBlocks` → `insert_blocks`, `testPatchBlock` → `patch_block`, `testPatchBlocks` → `patch_blocks`, `testDeleteBlock` → `delete_block`, `testUndeleteBlock` → `undelete_block`, `testGetSubTree2` → `get_sub_tree2`, `testGetBlocks` → `get_blocks`, `testGetBlock` → `get_block`, `testDuplicateBlock` → `duplicate_block`, `testGetBlockMetadata` → `get_block_metadata`, `testUndeleteBlockChildren` → `undelete_block_children` |
| `boards.go` | `testGetBoard` → `boards::get_board`, `testGetBoardsForUserAndTeam` → `get_boards_for_user_and_team`, `testGetBoardsInTeamByIds` → `get_boards_in_team_by_ids`, `testInsertBoard` → `insert_board`, `testPatchBoard` → `patch_board`, `testDeleteBoard` → `delete_board`, `testUndeleteBoard` → `undelete_board`, `testInsertBoardWithAdmin` → `insert_board_with_admin`, `testSaveMember` → `save_member`, `testGetMemberForBoard` → `get_member_for_board`, `testGetMembersForBoard` → `get_members_for_board`, `testGetMembersForUser` → `get_members_for_user`, `testDeleteMember` → `delete_member`, `testSearchBoardsForUser` → `search_boards_for_user`, `testSearchBoardsForUserInTeam` → `search_boards_for_user_in_team`, `testGetBoardHistory` → `get_board_history`, `testGetBoardCount` → `get_board_count` |
| `boards_and_blocks.go` | `testCreateBoardsAndBlocks` → `boards_and_blocks::create_boards_and_blocks`, `testPatchBoardsAndBlocks` → `patch_boards_and_blocks`, `testDeleteBoardsAndBlocks` → `delete_boards_and_blocks`, `testDuplicateBoard` → `duplicate_board` |
| `category.go` | `testGetCreateCategory` → `categories::get_create_category`, `testUpdateCategory` → `update_category`, `testDeleteCategory` → `delete_category`, `testGetUserCategories` → `get_user_categories`, `testReorderCategories` → `reorder_categories`, `testReorderCategoryBoards` → `reorder_category_boards` |
| `categoryBoards.go` | `testGetUserCategoryBoards` → `category_boards::get_user_category_boards`, `testAddUpdateCategoryBoard` → `add_update_category_board`, `testSetBoardVisibility` → `set_board_visibility` |
| `users.go` | `testGetUsersByTeam` → `users::get_users_by_team`, `testCreateAndGetUser` → `create_and_get_user`, `testGetUsersList` → `get_users_list`, `testCreateAndUpdateUser` → `create_and_update_user`, `testCreateAndGetRegisteredUserCount` → `create_and_get_registered_user_count`, `testPatchUserProps` → `patch_user_props` |
| `session.go` | `testCreateAndGetAndDeleteSession` → `sessions::create_and_get_and_delete_session`, `testGetActiveUserCount` → `get_active_user_count`, `testUpdateSession` → `update_session` |
| `system.go` | `testSetGetSystemSettings` → `system_settings::set_get_system_settings` |
| `teams.go` | `testGetTeam` → `teams::get_team`, `testUpsertTeamSignupToken` → `upsert_team_signup_token`, `testUpsertTeamSettings` → `upsert_team_settings`, `testGetAllTeams` → `get_all_teams` |
| `sharing.go` | `testUpsertSharingAndGetSharing` → `sharing::upsert_sharing_and_get_sharing` |
| `subscriptions.go` | `testCreateSubscription` → `subscriptions::create_subscription`, `testDeleteSubscription` → `delete_subscription`, `testUndeleteSubscription` → `undelete_subscription`, `testGetSubscription` → `get_subscription`, `testGetSubscriptions` → `get_subscriptions`, `testGetSubscribersForBlock` → `get_subscribers_for_block` |
| `files.go` | `StoreTestFileStore` → `file_info::file_store` |
| `cloud.go`, `compliance.go`, `data_retention.go`, `notificationhints.go` | 해당 없음: 그 메서드를 옮기지 않았다 |
| `blocks.go` `testGetBlockHistoryNewestChildren` | 해당 없음: 메서드를 옮기지 않았다 |

그대로 옮기지 못한 단언:

- **직렬화할 수 없는 값**(`"invalid fields data"`, `"invalid properties field board"`): 원본은 JSON 필드에 Go 함수 값을 넣어 직렬화를 실패시킨다. `serde_json` 맵은 그런 값을 담을 수 없어 케이스 자체가 성립하지 않는다(3곳, 테스트 주석에 표시).
- **부분 결과 + 오류**(의도적 차이 9): `get_blocks_by_ids`, `get_boards_in_team_by_ids`, `get_users_list`에서 원본이 오류와 함께 받은 목록의 길이를 검사하는 단언은 다른 조회로 같은 기대값을 확인한다.
- **`assert.Nil(결과)`와 오류를 함께 보는 단언**: `Result`는 둘을 함께 담지 않는다. 오류 쪽 단언은 유지했다.
- **`testSaveMember`**: 원본은 반환된 멤버를 검사한다. `save_member`는 값을 돌려주지 않아 같은 멤버를 `get_member_for_board`로 읽어 검사한다.
- **`testCreateSubscription/"invalid subscription"`**: 원본은 빈 구독자 타입에서 시작한다. enum에는 빈 값이 없어 3단계와 4단계가 같은 호출이다.
- **`time.Sleep`**: history 충돌 회피용 1ms 대기는 옮기지 않았다(의도적 차이 4). 타임스탬프 경계를 비교하는 테스트 두 곳에만 짧은 대기가 있다.
- **SQLite에서 건너뛰는 서브테스트**: 원본은 "실패하면 아무것도 저장되지 않아야 한다"는 서브테스트를 SQLite에서 건너뛴다(트랜잭션이 없으므로). 포트는 두 DB 모두에서 실행한다.

원본에 없어 새로 쓴 테스트(각 테스트에 "원본에 없음" 표시): 동시 쓰기(`blocks::concurrent_writers_on_one_board_all_succeed`, 16개 태스크), history 무충돌 3종, 파일을 가진 블록 삭제, 원본이 쓴 BLOB JSON 읽기, 페이징, 템플릿 조회·제거, 검색의 대소문자·와일드카드, 세션 만료·갱신·정리, 재정렬의 ID 바인딩, 스키마 생성·재접속·접두사 검사, `dbconfig` 변환 표(`src/connect.rs`의 단위 테스트) 등.

## 보존한 quirk

코드에 `// Preserved quirk:` 주석으로 표시되어 있다.

**블록 (`sqlstore/blocks.go`)**
- 새 블록은 호출자가 준 `create_at`이 아니라 현재 시각으로 저장되지만, 호출자의 구조체에는 반영되지 않는다(`:230-262`). 그래서 블록 생성 응답의 `createAt`이 요청 값 그대로다(오라클 확인).
- 갱신은 `id`와 `board_id`가 모두 맞아야 한다. 다른 보드 ID로 온 갱신은 블록을 건드리지 않지만 history 행은 쓴다(`:283`).
- 갱신의 history 행은 수정자를 작성자로, 수정 시각을 생성 시각으로 기록한다(`:274-276`, `:309`).
- 타입 필터가 빈 문자열이거나 `unknown`이면 필터하지 않는다(`:69`).
- 블록을 지우면 직계 자식만 함께 지운다. ID가 빈 문자열인 블록을 지우면 그 보드의 블록 전부가 지워진다(`:450`, `:929`).
- `get_blocks_by_ids`는 개수를 비교하므로 같은 ID를 두 번 주면 `NotAllFound`다(`:128`).
- 목록 질의에 `ORDER BY`가 없다.

**보드와 멤버 (`sqlstore/board.go`, `boards_and_blocks.go`, `templates.go`)**
- 없는 보드의 오류 문구는 `{boards} not found`다(`board.go:227`).
- 보드 갱신은 저장된 팀·작성자·생성 시각을 바꾸지 않지만 history 행에는 호출자가 준 값이 들어간다(`board.go:339-399`).
- 관리자 멤버십은 넘겨받은 보드의 `created_by`에게 간다. 기존 보드면 빈 값일 수 있다(`board.go:487-492`).
- 멤버의 `roles`는 저장하지 않는다(`board.go:503-532`). 보드를 지워도 멤버십은 남는다.
- 되살린 보드의 `channel_id`는 비어 있다(`board.go:838-842`).
- 제목 검색은 공백 하나로 단어를 나눠 빈 단어(공백 두 개)가 모든 보드와 일치하고, `%`와 `_`가 와일드카드로 동작한다. SQLite의 `lower()`는 ASCII만 접는다(`board.go:691-695`, `:736-740`).
- 속성 이름 검색은 SQLite에서 JSON 경로 `$.<검색어>`가 된다. `a.b`는 중첩 키와 일치하고 잘못된 경로는 DB 오류다(`board.go:678-680`).
- 전역 팀(`0`)의 템플릿은 제목의 MD5를 `trackingTemplateId` 속성으로 받는다(`board.go:299-303`).
- `RemoveDefaultTemplates`는 넘겨받은 보드의 `created_by`로 판단하고, 보드는 템플릿일 때만 지우지만 블록은 항상 지우며 history를 남기지 않는다(`templates.go:24-41`).
- 블록이 없거나 댓글만 있는 보드는 복제할 수 없다(`boards_and_blocks.go:177`). 복제된 카드의 `parent_id`가 원본 보드 ID이면 그대로 남는다.
- 삭제 요청에서 블록이 부모보다 뒤에 있으면 not-found로 실패한다(`boards_and_blocks.go:110-123`).

**카테고리 (`sqlstore/category.go`, `category_boards.go`)**
- 생성 직후의 순서 밀기가 새 행에도 적용되어, 정렬 순서 N으로 만든 카테고리는 N+10으로 저장된다(`category.go:96`).
- 카테고리 재정렬은 삭제된 카테고리도 옮긴다(`category.go:228`).
- 카테고리 안 보드 재정렬은 `ELSE`가 없어, 새 순서에 빠진 보드의 `sort_order`가 NULL이 된다. 정렬 값은 10의 배수가 아니라 `i + 10`이다(`category_boards.go:144-146`).
- 다른 카테고리로 옮긴 보드는 이전 정렬 순서와 숨김 상태를 유지한다(`category_boards.go:105`).
- `getCategoryBoardAttributes`는 `category_id`만으로 조회한다(`category_boards.go:42`). 서버 계층이 소유자를 먼저 확인한다(서버 PORTING.md의 보안 수정).
- 갱신·삭제·숨김 설정은 일치하는 행이 없어도 성공한다. `get_category`는 삭제된 카테고리도 돌려준다.

**사용자·세션·팀·공유·구독·파일 (`sqlstore/user.go`, `session.go`, `team.go`, `sharing.go`, `subscriptions.go`, `file.go`)**
- 없는 사용자의 오류 문구는 `{user} not found`뿐이다(`user.go:54,95`).
- `update_user`는 이름과 이메일만 쓴다. 비밀번호 갱신은 그 이름의 모든 사용자(삭제된 사용자 포함)에 적용된다(`user.go:144-173`).
- 사용자 검색은 이스케이프 없는 `LIKE` 하나, 최대 10명, 순서 없음(`user.go:227`). 사용자 이름·이메일 조회는 두 DB 모두 대소문자를 구분하고, 검색은 SQLite에서만 ASCII 대소문자를 무시한다.
- `get_session`은 `create_at`/`update_at`을 읽지 않아 항상 0이다. 그래서 인증 계층이 요청마다 세션을 갱신한다(`session.go:30-34`, `server/auth/auth.go:41`).
- 세션 만료·정리 인자는 초 단위이고, 정확히 경계에 있는 세션은 조회도 정리도 되지 않는다(`session.go:105-111`).
- 없는 세션·팀·공유의 오류 문구는 `sql: no rows in result set`다(`StoreError::NoRows`). 이 문구가 응답에 그대로 나간다.
- `upsert_team_settings`는 넘겨받은 가입 토큰을 무시하고, 새 팀이면 토큰을 새로 만든다(`team.go:58`).
- 이미 삭제된 구독을 지워도 성공한다. 다시 구독하면 저장된 `create_at`과 타입은 그대로인데 반환 값은 새 값이다(`subscriptions.go:82-104`).
- `save_file_info`는 항상 `archived = false`를 쓰고, 테이블에 키가 없어 중복 행이 생길 수 있다(`file.go:36`).

## 검증

2026-11-10, rustc 1.99.1, SQLite(번들), PostgreSQL 18.6(컨테이너 `hafgufa-pg`):

- `cargo fmt --check`: 통과
- `cargo clippy -p fvoci-store --all-targets --all-features -- -D warnings`: 통과
- `cargo test -p fvoci-store` (SQLite): 단위 5 + 통합 116 통과
- `FVOCI_STORE_TEST_DB_TYPE=postgres FVOCI_STORE_TEST_CONN_STRING=… cargo test -p fvoci-store --test store` (PostgreSQL, 테스트마다 스키마 분리): 116 통과
- 동시 쓰기 테스트 단독 반복: DB마다 20회 중 20회 통과
- 돌연변이 확인: `BEGIN IMMEDIATE`를 빼면 SQLite에서 동시 쓰기 테스트가 `database is locked`로 실패. history 타임스탬프를 일반 시계나 `now()`로 바꾸면 무충돌 테스트가 실패. `patch_boards_and_blocks`의 트랜잭션을 빼면 두 테스트가 실패. (모두 확인 후 원복)
- 원본이 쓴 SQLite DB 읽기: 오라클이 만든 DB 사본에서 사용자·세션·팀·설정·파일 정보·블록 372개를 스토어로 읽음
- 스키마 대조: 위 "스키마 대조" 절
- 미검증: 원본 Go 테스트 스위트는 실행하지 않았다. 원본이 쓴 PostgreSQL DB를 포트로 읽는 것과 포트가 만든 DB를 원본으로 여는 것은 서버 트랙 S5의 DB 왕복에서 확인한다
