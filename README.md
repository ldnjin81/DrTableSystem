# DrTableSystem

**DesignToRuntime Table System** — 기획자가 엑셀에 적은 데이터를 게임 런타임까지 그대로 옮긴다.

엑셀 한 벌에서 **서버 JSON · 클라이언트 JSON · 언리얼 C++ 코드**를 뽑는 데이터 생성기.

기획자가 엑셀에서 데이터를 관리하고, 생성기가 서버와 클라이언트가 쓸 산출물을 한 번에 만든다. 행 구조체와 열거형 코드도 생성하므로 스키마가 한 곳(엑셀)에만 존재한다.

```
Tables.xlsx ──┬─→ C++ (USTRUCT 행 구조체, UENUM 열거형, 테이블 목록)
              ├─→ 클라이언트 JSON (클라 범위 필드만)
              └─→ 서버 JSON (서버 범위 필드만)
```

## 사용

```powershell
uv sync
uv run drtable build --input Data\Tables.xlsx --out-cpp Generated --out-client Data\client --out-server Data\server
uv run drtable graph --input Data\Tables.xlsx --out Data\references.md
uv run drtable check --client Data\client --server Data\server
uv run drtable check --input Data\Tables.xlsx  # 기존 엑셀 스키마 검사
```

## 엑셀 규약 요약

- 시트 이름: `#`으로 시작하면 설명용(무시), `<enum>ItemType` 형태는 열거형 정의, 그 외는 테이블(시트명=테이블명). 열거형을 먼저 생성한다.
- 열거형 시트도 **헤더 3행 포맷이 같다**. 기본키가 열거자 이름이고, `Id`·`Value`뿐이면 코드만 생성한다. 부가 열이 있으면 `ItemTypeInfo` 같은 테이블도 함께 생성한다.
- 테이블 헤더는 3행: **필드명 / 자료형 / 범위**. 데이터는 4행부터.
- 키는 자료형을 감싸서 적는다: `ID<int32>`(기본키, 테이블마다 하나), `SubKey<name>`(서브키, 여러 개 가능). 일반 필드는 자료형만 쓴다.
- 문자열 계열 표기는 `name`·`string`·`text`·`tag`·`path`이며, C++에서는 각각 `FName`·`FString`·`FText`·`FGameplayTag`·`FSoftObjectPath`로 생성된다.
- 범위는 `B`(둘 다) · `C`(클라만) · `S`(서버만) · `#`(주석, 제외).
- `Reward[0]`, `Reward[1]` 처럼 번호를 붙인 열은 하나의 고정 배열 필드로 묶인다(`int32 Reward[N];`, JSON은 배열).
- `Ref<Items>`는 `Items` 테이블 기본키를 가리킨다. `Ref<DropTable.GroupId>`는 서브키로 묶인 1:N 행을 가리킨다. 관계는 manifest와 Mermaid 그래프로 나오고, 참조 값은 별도 `check` 명령으로 검사한다.
- 매니페스트는 기본적으로 시각을 넣지 않는다. 필요하면 `build --stamp <ISO8601>`로 결정적인 값을 명시한다.

자세한 규칙과 산출물 형식은 [docs/SPEC.md](docs/SPEC.md)에 있다.

- Codex 작업 규칙: [AGENTS.md](AGENTS.md)
- 설치 위치: PC `C:\tools\drtable`, 맥 `~/Project/drtable`
- 상태: v0.1 구현
