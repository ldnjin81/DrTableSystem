// Copyright ldnjin81. All Rights Reserved.

#pragma once

#include "CoreMinimal.h"
#include "TableGenAssetBase.h"
#include "TableGenTestTypes.generated.h"

// Hand-written stand-ins for generated code, used by the plugin's automation tests.

UENUM()
enum class ETableGenTestGroup : uint8
{
	// Declared out of name order on purpose: indices are sorted by value, not name.
	ZShared = 0,
	AOther = 1,
};

USTRUCT()
struct FTableGenTestRow
{
	GENERATED_BODY()

	UPROPERTY()
	int32 Id = 0;

	UPROPERTY()
	ETableGenTestGroup Group = ETableGenTestGroup::ZShared;

	UPROPERTY()
	FName Code;
};

UCLASS()
class UTableGenTestAsset : public UTableGenAssetBase
{
	GENERATED_BODY()

public:
	UPROPERTY()
	TArray<FTableGenTestRow> Rows;

	UPROPERTY()
	TArray<int32> PrimaryKeys;

	UPROPERTY()
	TArray<ETableGenTestGroup> Group_Keys;

	UPROPERTY()
	TArray<int32> Group_Offsets;

	UPROPERTY()
	TArray<int32> Group_Indices;

	UPROPERTY()
	TArray<FName> Code_Keys;

	UPROPERTY()
	TArray<int32> Code_Offsets;

	UPROPERTY()
	TArray<int32> Code_Indices;
};

UCLASS()
class UTableGenNoSubKeyTestAsset : public UTableGenAssetBase
{
	GENERATED_BODY()

public:
	UPROPERTY()
	TArray<FTableGenTestRow> Rows;

	UPROPERTY()
	TArray<int32> PrimaryKeys;
};
