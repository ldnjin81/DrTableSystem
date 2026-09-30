# DrTableSystem 매뉴얼

DrTableSystem(DesignToRuntime Table System)은 기획자가 엑셀에 적은 데이터를 게임 런타임까지 그대로 옮기는 테이블 시스템입니다. 테이블 **구조(스키마)**와 **데이터**를 나눠 관리하고, 다음을 만듭니다.

- **언리얼 C++**: `USTRUCT` 행 구조체, `UENUM` 열거형, 테이블마다 DataAsset 클래스 하나. 원하면 타입 안전 조회 함수와 등록 헤더까지. **스키마에서만 생성**하므로 기획이 데이터를 고쳐도 코드는 바뀌지 않습니다.
- **클라이언트 JSON**: 게임 클라이언트가 쓸 행과, 미리 계산한 키 인덱스. DataAsset으로 굽기 위한 입력입니다.
- **서버 JSON**: 서버가 쓸 행(서버 전용 필드 포함, 클라 전용 필드 제외).

**DrTableSystem 언리얼 플러그인**은 클라 JSON을 DataAsset으로 굽고, 런타임에 복사나 인덱스 구축 없이 그대로 읽으며, 오래된 에셋을 잡아냅니다.

```
스키마(Schemas/*.schema.xlsx, Enums/*.enum.xlsx) ─┐        ┌─▶ C++ 헤더 ─────────────▶ 컴파일
                                                    ├▶ drtable build ─┼─▶ 클라 JSON ─▶ DrTableBake ─▶ DA_*.uasset ─▶ 런타임 조회
데이터 엑셀(*.xlsx, 1행 필드명 + 4행부터 데이터) ─────┘        └─▶ 서버 JSON ─▶ 서버
                 drtable check  ◀─ 클라·서버 JSON   (참조 무결성 검사, CI)
                 drtable graph  ─▶ references.md   (Mermaid 다이어그램)
```

| 누가 | 무엇을 | 결과 |
|---|---|---|
| 프로그래머 | 스키마(필드·자료형·범위, 열거형 값) | 코드가 바뀜 → 리뷰 대상 |
| 기획자 | 데이터 엑셀(값, 행, 파일·시트 나누기) | JSON·에셋만 바뀜, 코드는 그대로 |

목차

1. [설치](#1-설치)
2. [스키마와 데이터](#2-스키마와-데이터)
3. [자료형](#3-자료형)
4. [기본키·서브키·기본값](#4-기본키서브키기본값)
5. [배열](#5-배열)
6. [열거형](#6-열거형)
7. [테이블 간 참조](#7-테이블-간-참조)
8. [산출물](#8-산출물)
9. [명령줄](#9-명령줄)
10. [언리얼 플러그인](#10-언리얼-플러그인)
11. [변경 감지: 스키마 해시와 내용 해시](#11-변경-감지-스키마-해시와-내용-해시)
12. [조회가 틀리지 않게 지키는 규칙](#12-조회가-틀리지-않게-지키는-규칙)
13. [CI](#13-ci)
14. [예전 형식에서 옮기기](#14-예전-형식에서-옮기기)
15. [문제 해결](#15-문제-해결)

---

## 1. 설치

필요한 것: Python 3.12 이상, [uv](https://docs.astral.sh/uv/)(권장). 실행 의존성은 `openpyxl` 하나입니다.

```sh
git clone <이 저장소> DrTableSystem
cd DrTableSystem
uv sync
uv run drtable --version
```

언리얼 쪽은 `unreal/DrTableSystem`을 프로젝트의 `Plugins/` 폴더에 복사합니다([10절](#10-언리얼-플러그인)). 플러그인은 Unreal Engine 5.8에서 개발·검증했습니다.

메시지는 기본이 영어입니다. 한국어로 보려면 `--lang ko`를 주거나 환경변수 `DRTABLE_LANG=ko`를 설정합니다. 생성되는 파일은 언제나 영어입니다.

## 2. 스키마와 데이터

### 폴더 구조

```
Design/Tables/                    ← --input (데이터)
  Items.xlsx
  Monsters.xlsx
  event/Summer.xlsx               ← 하위 폴더도 읽습니다
  Schemas/                        ← --schema (테이블 스키마)
    Items.schema.xlsx
    Monsters.schema.xlsx
    Enums/                        ← --enums (열거형 스키마, 기본값 <스키마>/Enums)
      ItemType.enum.xlsx
```

- 스키마는 테이블마다 파일 하나, 열거형은 열거형 폴더에 열거형마다 파일 하나입니다. 파일 이름은 정의한 이름과 같아야 합니다(`Items.schema.xlsx` ↔ 테이블 `Items`).
- `--schema`를 주지 않으면 입력 폴더 전체에서 `*.schema.xlsx`를 찾습니다. 데이터를 읽을 때 스키마 파일은 건너뜁니다.
- 엑셀이 열어 둔 파일의 잠금 파일(`~$…`)과 `.`으로 시작하는 숨김 폴더는 건너뜁니다.

### 스키마 파일

`Items.schema.xlsx`: 시트 하나에 테이블 이름을 붙이고, 1행은 제목, 2행부터 필드를 한 행에 하나씩 적습니다.

| | A 필드명 | B 자료형 | C 범위 | D 설명 |
|---|---|---|---|---|
| **1** | `Field` | `Type` | `Scope` | `Comment` |
| **2** | `Id` | `ID<int32>` | `all` | |
| **3** | `Name` | `SubKey<name>` | `all` | 표시 이름 |
| **4** | `Element` | `SubKey<EElement>` | `all` | |
| **5** | `Damage` | `float=0` | `client` | 초당 피해 |

- **자료형**에는 키 역할이나 기본값을 붙일 수 있습니다: `int32`, `ID<int32>`, `SubKey<name>`, `float=1.0`(3·4절).
- **범위**: `all` 둘 다, `client` 클라만, `server` 서버만, `#` 주석(어디에도 나가지 않음). 대소문자는 가리지 않습니다.
- 필드 순서가 곧 C++ 구조체의 멤버 순서입니다.

### 데이터 엑셀

| 행 | 내용 |
|---|---|
| 1 | 필드명. 빌드는 이 이름으로 스키마의 필드를 찾습니다. |
| 2·3 | 참고용(자료형·범위). **빌드는 읽지 않습니다.** 보통 스키마를 보여 주는 수식을 둡니다(아래 참고 헤더). |
| 4~ | 데이터 |

- **시트 이름이 테이블 이름**입니다. `#`으로 시작하는 시트는 메모이고, 이름 중간의 `#` 뒤는 주석입니다(`Items#무기`는 테이블 `Items`).
- **열 순서는 자유**입니다. 스키마의 필드 순서와 달라도 됩니다.
- 1행 이름이 `#`으로 시작하는 열은 메모입니다. 1행에서 빈 칸을 만나면 그 오른쪽 열은 읽지 않습니다. 데이터 칸이 전부 빈 행은 건너뜁니다.
- 스키마에 없는 필드명, 빠진 필드, 중복 필드명은 오류입니다. **필드 추가는 스키마에서** 합니다.
- 스키마만 있고 데이터 시트가 없는 테이블은 행 0개로 만들어집니다.

### 참고 헤더

`drtable headers`는 데이터 시트 2·3행에 스키마를 찾아 보여 주는 수식을 넣습니다. 1행 필드명으로 스키마 엑셀 파일을 찾아오므로, 스키마가 바뀌면 파일을 열 때 반영됩니다.

```
2행: =IFERROR(INDEX('[1]Items'!$B:$B,MATCH(A$1,'[1]Items'!$A:$A,0)),"(스키마에 없음)")
```

- 새 열을 추가하면 옆 칸 수식을 끌어 채우면 됩니다. 스키마에 없는 이름은 `(스키마에 없음)`으로 보입니다.
- 이 명령은 데이터 파일을 다시 저장합니다. 그림·차트·피벗이 있는 파일은 openpyxl이 보존하지 못하므로 건드리지 않고 알려 줍니다(수식은 손으로 복사).
- 처음 열 때 엑셀이 외부 링크 경고를 띄우면 "콘텐츠 사용"을 누르거나, 데이터 폴더를 엑셀의 **신뢰할 수 있는 위치**에 등록하세요.

**링크 경로 주의.** 엑셀은 스키마 파일이 데이터 파일과 **같은 폴더나 그 하위 폴더**에 있을 때만 링크를 상대 경로로 저장합니다. 그 밖(예: 데이터가 `event/` 하위 폴더에 있고 스키마가 위쪽 `Schemas/`에 있을 때)이면 저장한 사람의 절대 경로가 기록되어, 다른 경로에 저장소를 받은 사람에게는 스키마가 바뀌어도 마지막 저장 때 값이 계속 보입니다. 빌드에는 영향이 없고, 빌드가 이런 파일을 찾아 경고합니다(`참고 헤더가 이 PC에 없는 경로를 가리킵니다`). `drtable headers`를 다시 돌리면 이 PC 기준으로 다시 연결됩니다. 팀이 같은 경로에 저장소를 받으면 이 문제는 생기지 않습니다.

### 나뉜 테이블

행이 많거나 여러 사람이 나눠 관리하는 테이블은 여러 시트·파일로 나눌 수 있습니다. `#` 주석을 뺀 **테이블 이름이 같은 시트**는 모두 한 테이블로 합쳐지고, 모두 같은 스키마를 따릅니다.

```
Design/Tables/
  Items.xlsx           시트 Items#무기, Items#방어구
  event/Summer.xlsx    시트 Items#여름 이벤트
```

- 기본키 중복과 name 키 대소문자 검사는 시트 전체에 걸쳐 합니다. 오류에는 두 위치가 모두 나옵니다. 예: `[event/Summer.xlsx]Items#여름 이벤트!A5: 기본키 값 '3'이 중복되었습니다 (처음: [Items.xlsx]Items#무기!A4)`.
- 행은 기본키 순으로 정렬되므로, 나누기 전과 산출물이 같습니다.
- 테이블의 데이터가 어느 파일·시트에 있는지는 manifest의 `sources`에 기록됩니다(8절).

### 위치 표기

모든 오류 메시지는 `[파일]시트!셀` 위치로 시작합니다. 예: `[Items.xlsx]Items!A7`, `[Items.schema.xlsx]Items!B4`.

파일 경로는 입력 폴더(데이터)나 스키마 폴더 기준 상대 경로입니다.

## 3. 자료형

| 자료형 표기 | C++ | 클라 JSON | 서버 JSON |
|---|---|---|---|
| `int32`, `int64` | `int32`, `int64` | 숫자 | 숫자 |
| `float`, `double` | `float`, `double` | 숫자 | 숫자 |
| `bool` | `bool` | true/false | true/false |
| `name` | `FName` | 문자열 | 문자열 |
| `string` | `FString` | 문자열 | 문자열 |
| `text` | `FText` | 문자열 | 문자열 |
| `tag` | `FGameplayTag` | 문자열 | 문자열 |
| `path` | `FSoftObjectPath` | 문자열 | 문자열 |
| `E<열거형>` | `E<접두사><열거형>` | 열거자 이름 | 열거자 이름 |
| `Ref<테이블>` / `Ref<테이블.서브키>` | 대상 키의 자료형 | 대상과 같음 | 대상과 같음 |

- 서버가 언리얼이라고 가정하지 않으므로, 서버 JSON에서 `name`·`string`·`text`·`tag`·`path`는 모두 문자열입니다.
- `path`는 언제나 타입 없는 `FSoftObjectPath`입니다. 타입이 있는 포인터가 필요하면 런타임에 `TSoftObjectPtr<T>(Path)`로 바꿉니다.
- `bool`은 TRUE/FALSE, 1/0, 문자열 `true`/`false`를 받습니다.
- 중첩 구조체는 지원하지 않습니다. 배열(5절)이나 다른 테이블 참조(7절)를 씁니다.

## 4. 기본키·서브키·기본값

- **기본키** `ID<자료형>`: 테이블마다 정확히 하나, 범위는 반드시 `all`. 값은 중복되거나 비면 안 됩니다.
- **서브키** `SubKey<자료형>`: 개수 제한 없음. 서브키마다 인덱스를 미리 계산해 두므로 "`Element`가 `Fire`인 모든 행"을 전체 순회 없이 이진 탐색으로 찾습니다. 값이 중복돼도 됩니다.
- 키 자료형은 `int32`, `int64`, `name`, 열거형만 됩니다. 실수는 비교가 불안정하고, `bool`은 키로 의미가 없으며, `string`·`text`·`tag`·`path`는 생성기 정렬과 일치하는 비교가 없기 때문입니다.
- **기본값**: `float=1.0`, `bool=true`, `string=없음`처럼 씁니다. 빈 칸은 선언한 기본값을, 없으면 자료형 기본값(0, false, 빈 값, 첫 열거자)을 씁니다. 키에는 기본값을 줄 수 없습니다.

## 5. 배열

스키마에서 `필드[0]`, `필드[1]`, … 처럼 번호를 붙인 필드는 고정 크기 배열 필드 하나가 됩니다. 데이터 엑셀의 1행에도 같은 이름(`Reward[0]`…)으로 열을 둡니다.

| 필드명 | 자료형 | 범위 |
|---|---|---|
| `Id` | `ID<int32>` | `all` |
| `Reward[0]` | `int32` | `all` |
| `Reward[1]` | `int32` | `all` |
| `Reward[2]` | `int32` | `all` |

- C++: C 스타일 배열 `int32 Reward[3] = {};`. 힙 할당 없이 행 안에 들어갑니다.
- JSON: 실제 배열 `"Reward": [10, 20, 30]`.
- 번호는 0부터 빈틈없이 이어져야 하고, 원소는 자료형·범위가 같아야 합니다. 배열은 키가 될 수 없습니다.
- 언리얼은 C 스타일 배열을 블루프린트에 노출할 수 없어서 배열 속성은 `EditAnywhere`만 붙습니다.
- 원소마다 기본값을 따로 줄 수 있습니다(`int32=10`, `int32=20`).

## 6. 열거형

열거형의 값(열거자 목록)은 C++ `UENUM`이 되므로 **스키마에 둡니다**. 열거형 폴더(기본 `<스키마>/Enums`)에 열거형마다 파일 하나입니다.

`Enums/ItemType.enum.xlsx`, 시트 이름 `ItemType`:

| | A 이름 | B 값 | C 설명 |
|---|---|---|---|
| **1** | `Name` | `Value` | `Comment` |
| **2** | `Weapon` | `0` | 검, 활 |
| **3** | `Armor` | | 투구, 갑옷 |

- 값은 생략할 수 있습니다(첫 항목은 0, 그다음은 앞 값 + 1). uint8 범위이고 중복되면 안 됩니다.
- 설명은 생성 코드의 주석이 됩니다.
- 테이블에서는 `E<이름>`(`EItemType`)으로 씁니다.
- 열거형은 나눌 수 없습니다. 같은 이름이 두 번 정의되면 오류입니다.
- **열거자별 부가 데이터**(표시 이름, 아이콘 등)는 열거형을 기본키로 하는 일반 테이블로 둡니다. 예: `ItemTypeInfo.schema.xlsx`에 `Id: ID<EItemType>`, `DisplayName: text` … 그리고 데이터 엑셀에 `ItemTypeInfo` 시트.

## 7. 테이블 간 참조

`Ref<Items>` 필드에는 `Items` 테이블의 기본키 값을 넣습니다. 필드의 실제 자료형은 대상 키의 자료형이라, 대상 키 자료형을 바꾸면 참조 필드도 따라 바뀝니다.

```
Quests:   Id: ID<int32>   RewardItem: Ref<Items>   Next[0]: Ref<Quests>   Next[1]: Ref<Quests>
```

- **빈 칸은 "참조 없음"**입니다. 숫자 키는 `0`, `name` 키는 빈 이름이 들어갑니다. 열거형 키 테이블을 가리키는 참조는 비울 수 없습니다(열거형에는 "없음" 값이 없기 때문).
- 자기 참조와 테이블 간 순환은 허용합니다. 배열과 `SubKey<Ref<...>>`도 됩니다. `ID<Ref<...>>`와 참조의 기본값은 안 됩니다.

**서브키 참조.** `Ref<DropTable.GroupId>`는 서브키 `GroupId`가 그 값인 행들을 가리킵니다. 값 하나에 행 여러 개(1:N)입니다.

```
Monsters:   Id: ID<name>        DropGroup: Ref<DropTable.GroupId>
DropTable:  Id: ID<int32>       GroupId: SubKey<int32>      Item: Ref<Items>
```

- 점 뒤의 필드는 `SubKey<>`로 선언돼 있어야 합니다(인덱스가 있어야 하므로). 기본키는 `Ref<테이블>`로 씁니다.
- 참조 필드의 범위가 대상 서브키의 범위보다 넓으면 안 됩니다. 예를 들어 `all` 필드가 `client`에만 있는 서브키를 가리키면 서버 산출물에서 검사할 수 없습니다.
- 대상 서브키가 또 참조면 자료형을 연쇄로 따라갑니다. 끝내 정할 수 없는 순환은 전체 경로와 함께 오류로 알려 줍니다.

**검증.** `build`는 스키마만 봅니다(대상이 있는지, 자료형이 정해지는지). 값이 실제로 있는지는 생성된 JSON에 대해 `drtable check`가 검사합니다(9절). 생성 C++에는 `meta = (TableRef = "Items")`(서브키 참조면 `TableRefKey = "GroupId"`도)가 붙어 에디터 도구도 참조를 따라갈 수 있습니다.

## 8. 산출물

### C++

```
<out-cpp>/EDtElement.h          열거형마다 하나
<out-cpp>/DtEffectsRow.h        테이블마다 행 구조체
<out-cpp>/DtEffectsTable.h      테이블마다 DataAsset 클래스
<out-cpp>/DtGeneratedTables.h   이름·키·스키마 해시 상수
```

`--runtime-header`(또는 `--ue-plugin`)를 주면 다음도 만듭니다.

```
<out-cpp>/DtEffectsRow.cpp        조회·참조 함수 정의
<out-cpp>/DtTableRegistration.h   DtGeneratedTables::RegisterAll(Registry)
```

- 행 구조체 `F<접두사><테이블>Row`, 에셋 클래스 `U<접두사><테이블>Table`, 열거형 `E<접두사><열거형>`. `--prefix` 기본값은 `Dt`이니 프로젝트 접두사로 바꿔 쓰세요.
- 클라 필드(`all`, `client`)만 생성합니다.
- **생성 코드는 스키마에만 의존합니다.** 데이터 값, 데이터 파일 이름, 행 수는 코드에 들어가지 않으므로, 데이터를 고치거나 파일·시트를 나눠도 코드는 바이트 단위로 같습니다. 첫 줄 주석에는 스키마 파일이 적힙니다(`// … Source: Items.schema.xlsx`).
- 에셋 클래스에는 `Rows`(기본키 순 정렬), `PrimaryKeys`(같은 순서), 그리고 서브키마다 `<이름>_Keys`, `<이름>_Offsets`, `<이름>_Indices`(생성기가 계산한 CSR 인덱스)가 들어갑니다.

**행 함수**(`--runtime-header`를 줄 때). 일반 C++ 멤버이고 블루프린트에는 노출하지 않습니다.

| 함수 | 생기는 조건 | 반환 |
|---|---|---|
| `static const FRow* Find(키)` | 모든 테이블 | 행 또는 nullptr |
| `static TArray<const FRow*> FindBy<서브키>(키)` | 서브키마다 | 해당 행들 |
| `static TConstArrayView<FRow> GetAll()` | 모든 테이블 | 전체 행 |
| `const F대상Row* Get<필드>() const` | `Ref<대상>` 필드 | 참조한 행 또는 nullptr |
| `TArray<const F대상Row*> Get<필드>() const` | `Ref<대상.서브키>` 필드 | 참조한 행들 |
| `Get<필드>(int32 Index) const` | 참조 배열 | 위와 같되 원소 하나 |

참조가 비어 있으면 조회 없이 바로 nullptr나 빈 배열을 돌려줍니다. 생성할 함수 이름이 필드와 겹치면(예: 필드 이름이 `Find`) 생성 오류입니다.

생성 함수는 런타임 헤더가 제공하는 템플릿 세 개만 부릅니다. 언리얼 플러그인의 `DrTableRuntime.h`가 구현하고, 다른 환경이면 직접 구현해도 됩니다.

```cpp
namespace DrTableRuntime {
  template <typename TRow, typename TKey> const TRow* FindByKey(const TKey& Key);
  template <typename TRow, typename TKey> TArray<const TRow*> FindAllBySubKey(FName SubKeyName, const TKey& Key);
  template <typename TRow> TConstArrayView<TRow> GetAll();
}
```

`RegisterAll(Registry)`에 넘기는 레지스트리에는 `Register<행, 에셋>(FName 이름, TArray<행> 에셋::*행배열, TArray<키> 에셋::*키배열)`이 있어야 합니다. 그 반환값은 `WithSchemaHash(const TCHAR*)`와 `WithSubKey(FName, 키, 오프셋, 인덱스)`를 이어 부를 수 있어야 합니다.

### JSON

클라 `Effects.json`:

```json
{
  "table": "Effects",
  "schema_hash": "sha256:…",
  "content_hash": "sha256:…",
  "primary_key": "Id",
  "primary_keys": [1001, 1002],
  "sub_keys": [{"name": "Element", "field": "Element",
                "keys": ["Fire", "Water"], "offsets": [0, 1, 2], "indices": [0, 1]}],
  "rows": [{"Id": 1001, "Name": "Burn", "Element": "Fire"}, …]
}
```

서버 JSON은 인덱스가 없고 서버 필드가 들어간다는 점만 다릅니다. 폴더마다 있는 `manifest.json`에는 다음이 들어갑니다.

- 테이블 목록: 행 수, 스키마·내용 해시, 스키마 파일(`schema`), 데이터 위치(`sources`: 파일·시트·행 수)
- 열거형과 참조
- 굽기 도구가 쓰는 이름 규칙(`cpp_prefix`, `asset_name`)

산출물은 **결정적**입니다. 같은 입력이면 바이트까지 같습니다(LF 줄바꿈, 고정된 키 순서, `--stamp`를 주지 않는 한 시각 정보 없음). 출력 폴더는 쓰기 전에 비우므로 지운 테이블의 파일이 남지 않습니다.

## 9. 명령줄

```sh
drtable build --input <xlsx|폴더> --out-cpp <dir> --out-client <dir> --out-server <dir>
               [--schema <폴더>] [--enums <폴더>]
               [--prefix Dt] [--ue-plugin] [--asset-base <클래스> --asset-base-header <헤더.h>]
               [--runtime-header <헤더.h>] [--asset-name DA_{table}] [--stamp <ISO8601>]
drtable graph --input <xlsx|폴더> --out references.md [--schema …] [--enums …]
drtable check --client <클라 JSON 폴더> [--server <서버 JSON 폴더>]
drtable check --input <xlsx|폴더> [--schema …] [--enums …]   # 검사만 하고 아무것도 쓰지 않음
drtable headers --input <xlsx|폴더> [--schema …] [--enums …]  # 데이터 2·3행에 참고 수식
drtable migrate --input <xlsx|폴더> [--schema …] [--enums …] [--overwrite]
drtable [--lang en|ko] …
```

| 옵션 | 뜻 |
|---|---|
| `--schema` | 테이블 스키마 폴더. 기본값은 입력 폴더. |
| `--enums` | 열거형 스키마 폴더. 기본값은 `<스키마>/Enums`. |
| `--prefix` | C++ 타입 접두사(`Dt` → `FDtEffectsRow`). |
| `--ue-plugin` | DrTableSystem 플러그인용 설정: 에셋 기반 `UDrTableAssetBase`, 런타임 헤더 `DrTableRuntime.h`. 플러그인과 함께 쓸 때 권장합니다. |
| `--asset-base`, `--asset-base-header` | 에셋 클래스의 기반 클래스와 그 헤더. 헤더 없이 기반만 바꾸면 오류입니다(컴파일되지 않는 코드가 나오므로). |
| `--runtime-header` | 행 함수, `.cpp`, 등록 헤더를 생성합니다. |
| `--asset-name` | 등록과 굽기에 쓰는 에셋 이름 형식. `{table}`이 반드시 들어가야 합니다. |
| `--stamp` | manifest에 `generated_at`을 넣습니다(CI 추적용. 의도적으로 바이트 동일성이 깨집니다). |

- `graph`는 GitHub에서 바로 그려지는 Mermaid `flowchart`를 씁니다. 테이블마다 노드(키 자료형 표시), 참조마다 화살표를 그립니다. 화살표 라벨에는 필드명과 배열이면 `[N]`을 붙입니다. 참조 필드가 서브키면 `(SubKey)`, 서브키를 참조하면 `→ 키 1:N`도 붙습니다.
- `check --client/--server`는 생성된 JSON만 읽으므로 CI에서 돌릴 수 있습니다. 끊긴 참조를 전부 출력합니다(예: `Quests.Next[2002](0) = 9999 → Quests 테이블에 없음`). 빈 참조는 건너뛰고, 대상에 "참조 없음" 값(0이나 빈 이름)과 같은 키가 있으면 경고합니다.
- 경고(`경고: …`)는 표준 오류로 나가고 종료 코드에 영향을 주지 않습니다.

종료 코드: `build`·`graph`·`check --input`·`headers`·`migrate`는 0 성공, 1 검증 오류, 2 사용 오류. `check --client`는 0 통과, 1 끊긴 참조, 2 입력 오류.

## 10. 언리얼 플러그인

`unreal/DrTableSystem`에는 모듈이 두 개 있습니다.

- **DrTableRuntime**: `UDrTableAssetBase`, `TDrTableRowTable`, `UDrTableRegistry`(엔진 서브시스템 `UDrTableSubsystem`이 소유), `UDrTableSettings`, `DrTableRuntime` 조회 계약, `DRTABLE_AUTO_REGISTER`.
- **DrTableEditor**: `DrTableBake` 커맨드릿과 플러그인 자동화 테스트(`DrTable.*`).

### 설정

1. `unreal/DrTableSystem`을 `<프로젝트>/Plugins/DrTableSystem`에 복사하고 켭니다(`"Plugins": [{"Name": "DrTableSystem", "Enabled": true}]`).
2. 게임 모듈의 `PublicDependencyModuleNames`에 `"DrTableRuntime"`을 추가합니다.
3. 모듈 소스 폴더로 생성합니다.
   ```sh
   drtable build --input Design/Tables --schema Design/Tables/Schemas --prefix Gm --ue-plugin \
     --out-cpp Source/MyGame/TableData/Generated \
     --out-client Intermediate/DrTable/client --out-server Build/ServerData
   drtable check --client Intermediate/DrTable/client --server Build/ServerData
   ```
4. 그 모듈의 아무 `.cpp`에서 생성된 테이블을 한 번 등록합니다.
   ```cpp
   #include "DrTableRegistry.h"
   #include "TableData/Generated/GmTableRegistration.h"

   DRTABLE_AUTO_REGISTER(GmGeneratedTables::RegisterAll<UDrTableRegistry>);
   ```
5. 에디터를 빌드한 뒤 굽습니다.
   ```sh
   UnrealEditor-Cmd MyGame.uproject -run=DrTableBake -Input=Intermediate/DrTable/client -Out=/Game/Data
   ```
6. 행을 씁니다.
   ```cpp
   if (const FGmItemsRow* Sword = FGmItemsRow::Find(1001))
   {
       const FGmQuestsRow* Quest = Sword->GetQuest();          // Ref<Quests>
   }
   for (const FGmItemsRow* Weapon : FGmItemsRow::FindByKind(EGmItemType::Weapon)) { … }
   ```

기획이 데이터만 고쳤다면 3번의 `drtable build`와 5번 굽기만 다시 하면 됩니다. 코드가 바뀌지 않으므로 컴파일은 필요 없습니다.

### 로드

처음 조회할 때 레지스트리가 등록된 테이블을 모두 `<AssetRoot>/<에셋이름>.<에셋이름>`에서 로드합니다. `AssetRoot`는 프로젝트 설정 → 플러그인 → DrTable에서 정하며 기본값은 `/Game/Data`입니다. `ExtraAssets`로 에셋을 추가하거나, 같은 에셋 이름의 테이블 경로를 바꿀 수 있습니다. `bLoadOnFirstUse`를 끄면 `UDrTableRegistry::Get()->LoadAllTables()`를 직접 부릅니다.

행은 에셋에서 그대로 읽습니다. 복사도, 런타임 인덱스 구축도 없습니다. **조회로 받은 포인터와 뷰는 테이블을 다시 로드하기 전까지만 유효합니다**(`DrTable.Reload`나 에디터에서 다시 구울 때). 다시 로드한 뒤에는 보관하지 말고 새로 조회하세요.

콘솔 명령: `DrTable.Status`(테이블과 로드된 행 수), `DrTable.Reload`.

### 굽기

```
-run=DrTableBake -Input=<클라 JSON 폴더> [-Out=/Game/Data] [-Force] [-Verify]
```

- 클래스 이름과 에셋 이름은 manifest의 규칙을 따르므로 생성 코드와 언제나 맞습니다.
- 에셋이 없거나 해시가 다른 테이블만 저장합니다. 같은 데이터를 다시 저장해도 패키지 바이너리(GUID)는 바뀌므로, 바뀌지 않은 테이블은 건너뜁니다. `-Force`는 전부 다시 저장합니다.
- `-Verify`는 아무것도 저장하지 않고, 빠졌거나 오래된 에셋이 있으면 실패합니다.
- 생성 클래스가 에디터에 컴파일돼 있지 않은 테이블은 오류입니다. 생성 → 빌드 → 굽기 순서로 하세요.

### 직접 만든 런타임과 쓰기

생성기는 플러그인에 의존하지 않습니다. `--ue-plugin` 없이 쓰면 평범한 `UPrimaryDataAsset` 클래스만 나오고 조회 함수는 없습니다. `--runtime-header 내런타임.h`와 `--asset-base`를 주면, 8절의 계약을 구현한 자체 시스템에서 테이블을 다룰 수 있습니다.

## 11. 변경 감지: 스키마 해시와 내용 해시

테이블마다 해시가 두 개 있습니다.

| 해시 | 대상 | 기록되는 곳 | 어긋나면 |
|---|---|---|---|
| `schema_hash` | 필드, 자료형, 키 역할, 범위, 선언한 기본값, 테이블이 쓰는 열거형 | JSON, manifest, 생성 C++, 에셋 | 로드할 때 **오류**, 테이블 미로드 |
| `content_hash` | 해당 산출물의 행과 인덱스 | JSON, manifest, 에셋 | `DrTableBake -Verify`가 **실패** |

- 스키마를 바꾸고 다시 생성·빌드·굽기를 하지 않으면, 런타임이 옛 구조의 에셋을 거부합니다.
- 값만 바꾸고 굽지 않은 경우는 굽기 검증(`-Verify`)이 잡습니다. 내용 해시는 생성 코드에 넣지 않습니다(넣으면 데이터 수정이 코드를 바꾸기 때문). 배포 전과 CI에서 `-Verify`를 돌리세요.

## 12. 조회가 틀리지 않게 지키는 규칙

조회는 생성기가 정렬해 둔 배열을 이진 탐색하므로, 생성기의 정렬과 런타임 비교가 글자 그대로 일치해야 합니다.

- 숫자는 수치 순서.
- **열거형은 이름이 아니라 값 순서.**
- **name은 유니코드 코드포인트 순서, 대소문자 구분**(`FName::LexicalLess`는 대소문자를 무시하고 숫자 접미사를 수치로 비교해서 어긋나므로 쓰지 않습니다).

도구가 강제하는 결과:

- **대소문자만 다른** `name` 키·서브키 값(`Sword`와 `sword`)은 오류입니다. 언리얼 `FName`은 둘을 같은 이름으로 봅니다.
- 열거형 값을 바꾸면 그 열거형을 쓰는 모든 테이블의 스키마 해시가 바뀌어, 오래된 인덱스가 거부됩니다.
- 런타임 키 자료형은 정확히 일치해야 합니다. `int32` 키 테이블은 `int64` 키로 찾아지지 않습니다.

## 13. CI

전형적인 파이프라인:

```sh
drtable build … --ue-plugin
drtable check --client … --server …                     # 끊긴 참조가 있으면 실패
UnrealEditor-Cmd … -run=DrTableBake -Input=… -Verify    # 빠졌거나 오래된 에셋이 있으면 실패
```

`.github/workflows/ci.yml`이 push마다 파이썬 테스트와 린트를 돌립니다.

## 14. 예전 형식에서 옮기기

예전 형식은 데이터 시트의 1~3행에 필드명·자료형·범위를 적고, 열거형을 `<enum>이름` 시트에 두었습니다. `drtable migrate`가 새 형식으로 옮깁니다.

```sh
drtable migrate --input Design/Tables --schema Design/Tables/Schemas
drtable headers --input Design/Tables --schema Design/Tables/Schemas            # 2·3행을 참고 수식으로
```

- 테이블 시트의 헤더 → `<테이블>.schema.xlsx`. 같은 테이블이 여러 시트에 있으면 첫 시트(파일 경로 순, 파일 안에서는 시트 순)를 씁니다.
- `<enum>이름` 시트 → 열거형 폴더의 `<이름>.enum.xlsx`. `Id`·`Value`·`Comment` 말고 다른 열이 있으면 `<이름>Info` 테이블의 스키마와 **새 데이터 파일** `<이름>Info.xlsx`를 만듭니다.
- **기존 데이터 파일은 고치지 않습니다.** 옛 `<enum>` 시트는 빌드가 읽지 않고 경고만 하니 확인한 뒤 지우세요. 옛 2·3행은 빌드가 읽지 않으니 그대로 두거나 `drtable headers`로 참고 수식으로 바꿉니다.
- 이미 있는 스키마 파일은 건너뜁니다(`--overwrite`로 덮어쓰기).

## 15. 문제 해결

| 메시지 | 원인과 조치 |
|---|---|
| `스키마가 없습니다. 'X.schema.xlsx'에…` | 데이터 시트 이름에 맞는 스키마가 없습니다. 스키마를 만들거나(예전 형식이면 `drtable migrate`) `--schema` 경로를 확인하세요. |
| `필드 'X'이 스키마 …에 없습니다` | 데이터 1행에 스키마에 없는 이름이 있습니다. 오타를 고치거나 스키마에 필드를 추가하세요(프로그래머). 메모 열이면 이름을 `#`으로 시작하세요. |
| `필드 'X'의 열이 없습니다` | 스키마의 필드가 데이터 시트에 없습니다. 열을 추가하세요. |
| `열거형 값은 이제 열거형 스키마…에 정의합니다` (경고) | 옛 `<enum>` 시트입니다. 값은 열거형 폴더의 스키마에서 읽으니 시트를 지우세요. |
| `참고 헤더가 이 PC에 없는 경로를 가리킵니다` (경고) | 다른 경로에서 저장된 링크입니다. `drtable headers`로 다시 연결하세요(2절 링크 경로 주의). |
| `열거형 스키마는 열거형 폴더(…)에 두어야 합니다` | `*.enum.xlsx` 파일을 열거형 폴더로 옮기세요. |
| `Schema mismatch, re-bake required` | 옛 구조로 구운 에셋입니다. 생성 → 빌드 → 굽기. |
| `Class U…Table is not compiled into the editor` | `drtable build` 뒤에 에디터를 빌드하고 굽습니다. |
| `Table asset not found` | 등록은 됐지만 굽지 않았거나, 굽기와 로드의 `AssetRoot`·`--asset-name`이 다릅니다. |
| `Row type is registered for more than one table` | 같은 행 구조체로 두 번 등록했습니다. 테이블 ID로 조회하세요(`FindRowByKey<TRow>(TableId, Key)`). |
| `--asset-base를 바꾸면 --asset-base-header도 필요합니다` | 기반 클래스를 선언한 헤더를 주거나 `--ue-plugin`을 쓰세요. |
| `생성할 함수 'Find'이 같은 이름의 필드와 겹칩니다` | 필드 이름을 바꾸세요. `Find`, `GetAll`, `FindBy<서브키>`, `Get<필드>`는 생성 이름입니다. |
| `name 키 'x'이 … 'X'과 대소문자만 다릅니다` | 철자를 똑같이 맞추거나 다른 이름을 쓰세요. |
| `옛 범위 표기 'B' 대신 'all'를 쓰세요` | 범위는 `all`·`client`·`server`·`#`만 씁니다. 옛 `B`·`C`·`S`는 각각 `all`·`client`·`server`로 바꾸세요. |
