# ue-tablegen 매뉴얼

ue-tablegen은 엑셀 테이블에서 다음을 만듭니다.

- **언리얼 C++**: `USTRUCT` 행 구조체, `UENUM` 열거형, 테이블마다 DataAsset 클래스 하나. 원하면 타입 안전 조회 함수와 등록 헤더까지.
- **클라이언트 JSON**: 게임 클라이언트가 쓸 행과, 미리 계산한 키 인덱스. DataAsset으로 굽기 위한 입력입니다.
- **서버 JSON**: 서버가 쓸 행(서버 전용 필드 포함, 클라 전용 필드 제외).

**TableGen 언리얼 플러그인**은 클라 JSON을 DataAsset으로 굽고, 런타임에 복사나 인덱스 구축 없이 그대로 읽으며, 오래된 에셋을 잡아냅니다.

스키마와 데이터의 원본은 엑셀 한 곳뿐입니다.

```
GameData.xlsx ─▶ tablegen build ─┬─▶ C++ 헤더 ─────────────▶ 컴파일
                                 ├─▶ 클라 JSON ─▶ TableGenBake ─▶ DA_*.uasset ─▶ 런타임 조회
                                 └─▶ 서버 JSON ─▶ 서버
                 tablegen check  ◀─ 클라·서버 JSON   (참조 무결성 검사, CI)
                 tablegen graph  ─▶ references.md   (Mermaid 다이어그램)
```

목차

1. [설치](#1-설치)
2. [엑셀 형식](#2-엑셀-형식)
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
14. [문제 해결](#14-문제-해결)

---

## 1. 설치

필요한 것: Python 3.12 이상, [uv](https://docs.astral.sh/uv/)(권장). 실행 의존성은 `openpyxl` 하나입니다.

```sh
git clone <이 저장소> ue-tablegen
cd ue-tablegen
uv sync
uv run tablegen --version
```

언리얼 쪽은 `unreal/TableGen`을 프로젝트의 `Plugins/` 폴더에 복사합니다([10절](#10-언리얼-플러그인)). 플러그인은 Unreal Engine 5.8에서 개발·검증했습니다.

메시지는 기본이 영어입니다. 한국어로 보려면 `--lang ko`를 주거나 환경변수 `TABLEGEN_LANG=ko`를 설정합니다. 생성되는 파일은 언제나 영어입니다.

## 2. 엑셀 형식

### 시트

| 시트 이름 | 뜻 |
|---|---|
| `#`으로 시작 | 메모. 읽지 않습니다. |
| `<enum>이름` | 열거형 정의(6절). |
| 그 밖 | 테이블. 시트 이름이 곧 테이블 이름입니다. |

테이블·열거형 이름은 C++ 식별자가 되므로 영문자로 시작하고 영문자·숫자·`_`만 써야 합니다. `--input`에는 `.xlsx` 하나 또는 폴더를 줄 수 있습니다. 폴더면 안의 `.xlsx`를 모두 읽고, 엑셀이 열어 둔 파일의 잠금 파일(`~$…`)은 건너뜁니다.

### 헤더 3행

| 행 | 내용 |
|---|---|
| 1 | 범위: `all` 둘 다, `client` 클라만, `server` 서버만, `#` 주석(어디에도 나가지 않음). 대소문자는 가리지 않습니다. |
| 2 | 자료형. 키 역할이나 기본값을 붙일 수 있습니다: `int32`, `ID<int32>`, `SubKey<name>`, `float=1.0` |
| 3 | 필드명(C++ 식별자). 데이터 바로 위에 있어 표를 읽기 쉽습니다. |
| 4~ | 데이터 |

필드명(3행)에서 빈 칸을 만나면 헤더가 끝나고, 그 오른쪽 열은 읽지 않습니다. 데이터 칸이 전부 빈 행은 건너뜁니다.

예시 시트 `Effects`:

| | A | B | C | D |
|---|---|---|---|---|
| **1** | `all` | `all` | `all` | `client` |
| **2** | `ID<int32>` | `SubKey<name>` | `SubKey<EElement>` | `float` |
| **3** | `Id` | `Name` | `Element` | `Damage` |
| **4** | `1001` | `Burn` | `Fire` | `12.5` |
| **5** | `1002` | `Freeze` | `Water` | `0` |

모든 오류 메시지는 시트와 셀 주소로 시작합니다. 예: `Effects!C7: 기본키 값 '1001'이 중복되었습니다`.

## 3. 자료형

| 2행 표기 | C++ | 클라 JSON | 서버 JSON |
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

`필드[0]`, `필드[1]`, … 처럼 번호를 붙인 열은 고정 크기 배열 필드 하나가 됩니다.

| | A | B | C | D |
|---|---|---|---|---|
| **1** | `all` | `all` | `all` | `all` |
| **2** | `ID<int32>` | `int32` | `int32` | `int32` |
| **3** | `Id` | `Reward[0]` | `Reward[1]` | `Reward[2]` |

- C++: C 스타일 배열 `int32 Reward[3] = {};`. 힙 할당 없이 행 안에 들어갑니다.
- JSON: 실제 배열 `"Reward": [10, 20, 30]`.
- 번호는 0부터 빈틈없이 이어져야 하고, 원소는 자료형·범위가 같아야 합니다. 배열은 키가 될 수 없습니다.
- 언리얼은 C 스타일 배열을 블루프린트에 노출할 수 없어서 배열 속성은 `EditAnywhere`만 붙습니다.
- 열마다 기본값을 따로 줄 수 있습니다(`int32=10`, `int32=20`).

## 6. 열거형

열거형 시트도 헤더 3행 형식이 같고, 기본키 열의 값이 열거자 이름입니다.

시트 `<enum>ItemType`:

| | A | B | C |
|---|---|---|---|
| **1** | `all` | `all` | `#` |
| **2** | `ID<name>` | `int32` | `string` |
| **3** | `Id` | `Value` | `Comment` |
| **4** | `Weapon` | `0` | `검, 활` |
| **5** | `Armor` | `1` | `투구, 갑옷` |

- `Value`는 생략할 수 있습니다. 비어 있으면 위에서부터 0, 1, 2…를 매깁니다. 값은 uint8 범위이고 중복되면 안 됩니다.
- 범위가 `#`이고 이름이 `Comment`인 열은 생성 코드의 주석이 됩니다.
- **`Id`와 `Value`뿐이면** 열거형만 만듭니다(`EDtItemType`).
- **다른 열이 더 있으면** 열거형과 함께, 열거형을 키로 하는 테이블 `ItemTypeInfo`를 만듭니다.

테이블에서는 `E<이름>`(`EItemType`)으로 씁니다.

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

**검증.** `build`는 스키마만 봅니다(대상이 있는지, 자료형이 정해지는지). 값이 실제로 있는지는 생성된 JSON에 대해 `tablegen check`가 검사합니다(9절). 생성 C++에는 `meta = (TableRef = "Items")`(서브키 참조면 `TableRefKey = "GroupId"`도)가 붙어 에디터 도구도 참조를 따라갈 수 있습니다.

## 8. 산출물

### C++

```
<out-cpp>/EDtElement.h          열거형마다 하나
<out-cpp>/DtEffectsRow.h        테이블마다 행 구조체
<out-cpp>/DtEffectsTable.h      테이블마다 DataAsset 클래스
<out-cpp>/DtGeneratedTables.h   이름·키·스키마 해시·내용 해시 상수
```

`--runtime-header`(또는 `--ue-plugin`)를 주면 다음도 만듭니다.

```
<out-cpp>/DtEffectsRow.cpp        조회·참조 함수 정의
<out-cpp>/DtTableRegistration.h   DtGeneratedTables::RegisterAll(Registry)
```

- 행 구조체 `F<접두사><테이블>Row`, 에셋 클래스 `U<접두사><테이블>Table`, 열거형 `E<접두사><열거형>`. `--prefix` 기본값은 `Dt`이니 프로젝트 접두사로 바꿔 쓰세요.
- 클라 필드(`all`, `client`)만 생성합니다.
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

생성 함수는 런타임 헤더가 제공하는 템플릿 세 개만 부릅니다. 언리얼 플러그인의 `TableGenRuntime.h`가 구현하고, 다른 환경이면 직접 구현해도 됩니다.

```cpp
namespace TableGenRuntime {
  template <typename TRow, typename TKey> const TRow* FindByKey(const TKey& Key);
  template <typename TRow, typename TKey> TArray<const TRow*> FindAllBySubKey(FName SubKeyName, const TKey& Key);
  template <typename TRow> TConstArrayView<TRow> GetAll();
}
```

`RegisterAll(Registry)`에 넘기는 레지스트리에는 `Register<행, 에셋>(FName 이름, TArray<행> 에셋::*행배열, TArray<키> 에셋::*키배열)`이 있어야 합니다. 그 반환값은 `WithSchemaHash(const TCHAR*)`, `WithContentHash(const TCHAR*)`, `WithSubKey(FName, 키, 오프셋, 인덱스)`를 이어 부를 수 있어야 합니다.

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

서버 JSON은 인덱스가 없고 서버 필드가 들어간다는 점만 다릅니다. 폴더마다 있는 `manifest.json`에는 테이블 목록(행 수, 스키마·내용 해시), 열거형, 참조, 그리고 굽기 도구가 쓰는 이름 규칙(`cpp_prefix`, `asset_name`)이 들어갑니다.

산출물은 **결정적**입니다. 같은 입력이면 바이트까지 같습니다(LF 줄바꿈, 고정된 키 순서, `--stamp`를 주지 않는 한 시각 정보 없음). 출력 폴더는 쓰기 전에 비우므로 지운 테이블의 파일이 남지 않습니다.

## 9. 명령줄

```sh
tablegen build --input <xlsx|폴더> --out-cpp <dir> --out-client <dir> --out-server <dir>
               [--prefix Dt] [--ue-plugin] [--asset-base <클래스> --asset-base-header <헤더.h>]
               [--runtime-header <헤더.h>] [--asset-name DA_{table}] [--stamp <ISO8601>]
tablegen graph --input <xlsx|폴더> --out references.md
tablegen check --client <클라 JSON 폴더> [--server <서버 JSON 폴더>]
tablegen check --input <xlsx|폴더>          # 검사만 하고 아무것도 쓰지 않음
tablegen [--lang en|ko] …
```

| 옵션 | 뜻 |
|---|---|
| `--prefix` | C++ 타입 접두사(`Dt` → `FDtEffectsRow`). |
| `--ue-plugin` | TableGen 플러그인용 설정: 에셋 기반 `UTableGenAssetBase`, 런타임 헤더 `TableGenRuntime.h`. 플러그인과 함께 쓸 때 권장합니다. |
| `--asset-base`, `--asset-base-header` | 에셋 클래스의 기반 클래스와 그 헤더. 헤더 없이 기반만 바꾸면 오류입니다(컴파일되지 않는 코드가 나오므로). |
| `--runtime-header` | 행 함수, `.cpp`, 등록 헤더를 생성합니다. |
| `--asset-name` | 등록과 굽기에 쓰는 에셋 이름 형식. `{table}`이 반드시 들어가야 합니다. |
| `--stamp` | manifest에 `generated_at`을 넣습니다(CI 추적용. 의도적으로 바이트 동일성이 깨집니다). |

`graph`는 GitHub에서 바로 그려지는 Mermaid `flowchart`를 씁니다. 테이블마다 노드(키 자료형 표시), 참조마다 화살표를 그립니다. 화살표 라벨에는 필드명과 배열이면 `[N]`을 붙입니다. 참조 필드가 서브키면 `(SubKey)`, 서브키를 참조하면 `→ 키 1:N`도 붙습니다.

`check --client/--server`는 생성된 JSON만 읽으므로 CI에서 돌릴 수 있습니다. 끊긴 참조를 전부 출력합니다(예: `Quests.Next[2002](0) = 9999 → Quests 테이블에 없음`). 빈 참조는 건너뛰고, 대상에 "참조 없음" 값(0이나 빈 이름)과 같은 키가 있으면 경고합니다.

종료 코드: `build`·`graph`·`check --input`은 0 성공, 1 검증 오류, 2 사용 오류. `check --client`는 0 통과, 1 끊긴 참조, 2 입력 오류.

## 10. 언리얼 플러그인

`unreal/TableGen`에는 모듈이 두 개 있습니다.

- **TableGenRuntime**: `UTableGenAssetBase`, `TTableGenRowTable`, `UTableGenRegistry`(엔진 서브시스템 `UTableGenSubsystem`이 소유), `UTableGenSettings`, `TableGenRuntime` 조회 계약, `TABLEGEN_AUTO_REGISTER`.
- **TableGenEditor**: `TableGenBake` 커맨드릿과 플러그인 자동화 테스트(`TableGen.*`).

### 설정

1. `unreal/TableGen`을 `<프로젝트>/Plugins/TableGen`에 복사하고 켭니다(`"Plugins": [{"Name": "TableGen", "Enabled": true}]`).
2. 게임 모듈의 `PublicDependencyModuleNames`에 `"TableGenRuntime"`을 추가합니다.
3. 모듈 소스 폴더로 생성합니다.
   ```sh
   tablegen build --input Design/GameData.xlsx --prefix Gm --ue-plugin \
     --out-cpp Source/MyGame/TableData/Generated \
     --out-client Intermediate/TableGen/client --out-server Build/ServerData
   tablegen check --client Intermediate/TableGen/client --server Build/ServerData
   ```
4. 그 모듈의 아무 `.cpp`에서 생성된 테이블을 한 번 등록합니다.
   ```cpp
   #include "TableGenRegistry.h"
   #include "TableData/Generated/GmTableRegistration.h"

   TABLEGEN_AUTO_REGISTER(GmGeneratedTables::RegisterAll<UTableGenRegistry>);
   ```
5. 에디터를 빌드한 뒤 굽습니다.
   ```sh
   UnrealEditor-Cmd MyGame.uproject -run=TableGenBake -Input=Intermediate/TableGen/client -Out=/Game/Data
   ```
6. 행을 씁니다.
   ```cpp
   if (const FGmItemsRow* Sword = FGmItemsRow::Find(1001))
   {
       const FGmQuestsRow* Quest = Sword->GetQuest();          // Ref<Quests>
   }
   for (const FGmItemsRow* Weapon : FGmItemsRow::FindByKind(EGmItemType::Weapon)) { … }
   ```

### 로드

처음 조회할 때 레지스트리가 등록된 테이블을 모두 `<AssetRoot>/<에셋이름>.<에셋이름>`에서 로드합니다. `AssetRoot`는 프로젝트 설정 → 플러그인 → TableGen에서 정하며 기본값은 `/Game/Data`입니다. `ExtraAssets`로 에셋을 추가하거나, 같은 에셋 이름의 테이블 경로를 바꿀 수 있습니다. `bLoadOnFirstUse`를 끄면 `UTableGenRegistry::Get()->LoadAllTables()`를 직접 부릅니다.

행은 에셋에서 그대로 읽습니다. 복사도, 런타임 인덱스 구축도 없습니다. **조회로 받은 포인터와 뷰는 테이블을 다시 로드하기 전까지만 유효합니다**(`TableGen.Reload`나 에디터에서 다시 구울 때). 다시 로드한 뒤에는 보관하지 말고 새로 조회하세요.

콘솔 명령: `TableGen.Status`(테이블과 로드된 행 수), `TableGen.Reload`.

### 굽기

```
-run=TableGenBake -Input=<클라 JSON 폴더> [-Out=/Game/Data] [-Force] [-Verify]
```

- 클래스 이름과 에셋 이름은 manifest의 규칙을 따르므로 생성 코드와 언제나 맞습니다.
- 에셋이 없거나 해시가 다른 테이블만 저장합니다. 같은 데이터를 다시 저장해도 패키지 바이너리(GUID)는 바뀌므로, 바뀌지 않은 테이블은 건너뜁니다. `-Force`는 전부 다시 저장합니다.
- `-Verify`는 아무것도 저장하지 않고, 빠졌거나 오래된 에셋이 있으면 실패합니다.
- 생성 클래스가 에디터에 컴파일돼 있지 않은 테이블은 오류입니다. 생성 → 빌드 → 굽기 순서로 하세요.

### 직접 만든 런타임과 쓰기

생성기는 플러그인에 의존하지 않습니다. `--ue-plugin` 없이 쓰면 평범한 `UPrimaryDataAsset` 클래스만 나오고 조회 함수는 없습니다. `--runtime-header 내런타임.h`와 `--asset-base`를 주면, 8절의 계약을 구현한 자체 시스템에서 테이블을 다룰 수 있습니다.

## 11. 변경 감지: 스키마 해시와 내용 해시

테이블마다 해시가 두 개 있고, JSON·manifest·생성 C++·(굽기 후) 에셋에 기록됩니다.

| 해시 | 대상 | 달라지는 경우 | 로드할 때 |
|---|---|---|---|
| `schema_hash` | 필드, 자료형, 키 역할, 범위, 선언한 기본값, 테이블이 쓰는 열거형 | 구조가 바뀜 | **오류**, 테이블 미로드 |
| `content_hash` | 해당 산출물의 행과 인덱스 | 값이 하나라도 바뀜 | **경고**, 테이블은 로드 |

그래서 "엑셀 구조를 바꾸고 다시 빌드·굽기를 안 함"도, "값만 바꾸고 굽기를 안 함"도 옛 데이터를 조용히 쓰는 대신 알려 줍니다. 배포 전에는 `TableGenBake -Verify`로 둘 다 잡을 수 있습니다.

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
tablegen build … --ue-plugin
tablegen check --client … --server …                     # 끊긴 참조가 있으면 실패
UnrealEditor-Cmd … -run=TableGenBake -Input=… -Verify    # 빠졌거나 오래된 에셋이 있으면 실패
```

`.github/workflows/ci.yml`이 push마다 파이썬 테스트와 린트를 돌립니다.

## 14. 문제 해결

| 메시지 | 원인과 조치 |
|---|---|
| `Schema mismatch, re-bake required` | 옛 구조로 구운 에셋입니다. 생성 → 빌드 → 굽기. |
| `Data changed since the asset was baked` | 엑셀 값이 바뀌었습니다. 굽기를 돌리세요(바뀐 테이블만 저장됩니다). |
| `Class U…Table is not compiled into the editor` | `tablegen build` 뒤에 에디터를 빌드하고 굽습니다. |
| `Table asset not found` | 등록은 됐지만 굽지 않았거나, 굽기와 로드의 `AssetRoot`·`--asset-name`이 다릅니다. |
| `Row type is registered for more than one table` | 같은 행 구조체로 두 번 등록했습니다. 테이블 ID로 조회하세요(`FindRowByKey<TRow>(TableId, Key)`). |
| `--asset-base를 바꾸면 --asset-base-header도 필요합니다` | 기반 클래스를 선언한 헤더를 주거나 `--ue-plugin`을 쓰세요. |
| `생성할 함수 'Find'이 같은 이름의 필드와 겹칩니다` | 필드 이름을 바꾸세요. `Find`, `GetAll`, `FindBy<서브키>`, `Get<필드>`는 생성 이름입니다. |
| `name 키 'x'이 … 'X'과 대소문자만 다릅니다` | 철자를 똑같이 맞추거나 다른 이름을 쓰세요. |
