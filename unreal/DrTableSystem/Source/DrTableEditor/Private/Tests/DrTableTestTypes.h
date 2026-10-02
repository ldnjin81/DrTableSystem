// Copyright ldnjin81. All Rights Reserved.

#pragma once

#include "CoreMinimal.h"
#include "DrTableAssetBase.h"
#include "DrTableTestTypes.generated.h"

// Hand-written stand-ins for generated code, used by the plugin's automation tests.

UENUM()
enum class EDrTableTestGroup : uint8
{
	// Declared out of name order on purpose: indices are sorted by value, not name.
	ZShared = 0,
	AOther = 1,
};

USTRUCT()
struct FDrTableTestRow
{
	GENERATED_BODY()

	UPROPERTY()
	int32 Id = 0;

	UPROPERTY()
	EDrTableTestGroup Group = EDrTableTestGroup::ZShared;

	UPROPERTY()
	FName Code;

	UPROPERTY()
	int32 Reward_Start = 0;

	UPROPERTY()
	int32 Reward_Num = 0;
};

UCLASS()
class UDrTableTestAsset : public UDrTableAssetBase
{
	GENERATED_BODY()

public:
	UPROPERTY()
	TArray<FDrTableTestRow> Rows;

	UPROPERTY()
	TArray<int32> PrimaryKeys;

	UPROPERTY()
	TArray<EDrTableTestGroup> Group_Keys;

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

	UPROPERTY()
	TArray<int32> Reward_Pool;
};

UCLASS()
class UDrTableNoSubKeyTestAsset : public UDrTableAssetBase
{
	GENERATED_BODY()

public:
	UPROPERTY()
	TArray<FDrTableTestRow> Rows;

	UPROPERTY()
	TArray<int32> PrimaryKeys;
};
