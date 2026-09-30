// Copyright ldnjin81. All Rights Reserved.

#pragma once

#include "CoreMinimal.h"
#include "Engine/DeveloperSettings.h"
#include "DrTableSettings.generated.h"

class UPrimaryDataAsset;

/** Project Settings > Plugins > DrTable. Stored in DefaultGame.ini. */
UCLASS(config = Game, defaultconfig, meta = (DisplayName = "DrTable"))
class DRTABLERUNTIME_API UDrTableSettings : public UDeveloperSettings
{
	GENERATED_BODY()

public:
	UDrTableSettings();

	/**
	 * Content folder that holds the baked table assets. Every registered table is loaded
	 * from <AssetRoot>/<TableId>.<TableId>; the table id is the asset name the generator
	 * wrote into the registration (--asset-name, default DA_{table}).
	 */
	UPROPERTY(EditAnywhere, config, Category = "Loading")
	FString AssetRoot;

	/**
	 * Optional extra or overriding assets. An entry whose asset name equals a registered
	 * table id replaces the automatic path for that table.
	 */
	UPROPERTY(EditAnywhere, config, Category = "Loading")
	TArray<TSoftObjectPtr<UPrimaryDataAsset>> ExtraAssets;

	/** Load all tables the first time any table is queried (otherwise call UDrTableRegistry::LoadAllTables yourself). */
	UPROPERTY(EditAnywhere, config, Category = "Loading")
	bool bLoadOnFirstUse = true;

	/** Warn when a baked asset's content hash differs from the generated code (spreadsheet changed, asset not re-baked). */
	UPROPERTY(EditAnywhere, config, Category = "Validation")
	bool bWarnOnContentMismatch = true;

	virtual FName GetCategoryName() const override { return TEXT("Plugins"); }
};
