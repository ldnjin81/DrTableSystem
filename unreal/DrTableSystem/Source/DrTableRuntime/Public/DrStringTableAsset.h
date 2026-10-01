// Copyright ldnjin81. All Rights Reserved.

#pragma once

#include "CoreMinimal.h"
#include "Engine/DataAsset.h"
#include "DrStringTableAsset.generated.h"

/**
 * One language of one string table, baked by DrTableBake from Strings/<language>/<Table>.json.
 * Only the assets of the current language are loaded (see UDrStringTables).
 */
UCLASS(BlueprintType)
class DRTABLERUNTIME_API UDrStringTableAsset : public UPrimaryDataAsset
{
	GENERATED_BODY()

public:
	UPROPERTY(VisibleAnywhere, BlueprintReadOnly, Category = "DrTable|Strings")
	FName Table;

	/** Culture code, e.g. ko, en, zh-Hans. */
	UPROPERTY(VisibleAnywhere, BlueprintReadOnly, Category = "DrTable|Strings")
	FString Language;

	UPROPERTY(VisibleAnywhere, BlueprintReadOnly, Category = "DrTable|Strings")
	FString SchemaHash;

	/** Hash of this language's keys and texts; the bake skips unchanged languages with it. */
	UPROPERTY(VisibleAnywhere, BlueprintReadOnly, Category = "DrTable|Strings")
	FString ContentHash;

	/** Keys, sorted. Values holds the text of each key in this language (empty translations were filled from the base language at build time). */
	UPROPERTY(VisibleAnywhere, Category = "DrTable|Strings")
	TArray<FName> Keys;

	UPROPERTY(VisibleAnywhere, Category = "DrTable|Strings")
	TArray<FString> Values;
};

/** The string table assets to load for one language. */
USTRUCT(BlueprintType)
struct DRTABLERUNTIME_API FDrStringLanguage
{
	GENERATED_BODY()

	UPROPERTY(VisibleAnywhere, BlueprintReadOnly, Category = "DrTable|Strings")
	FString Language;

	/** One asset per string table, sorted by table name. A table without this language uses its base language asset. */
	UPROPERTY(VisibleAnywhere, BlueprintReadOnly, Category = "DrTable|Strings")
	TArray<TSoftObjectPtr<UDrStringTableAsset>> Tables;
};

/**
 * Which languages exist and which assets make up each (<AssetRoot>/Strings/DA_DrStrings).
 * It only holds soft references, so loading it loads no text.
 */
UCLASS(BlueprintType)
class DRTABLERUNTIME_API UDrStringManifest : public UPrimaryDataAsset
{
	GENERATED_BODY()

public:
	/** The language used when no saved choice or engine culture matches: the base language of the string tables. */
	UPROPERTY(VisibleAnywhere, BlueprintReadOnly, Category = "DrTable|Strings")
	FString DefaultLanguage;

	UPROPERTY(VisibleAnywhere, BlueprintReadOnly, Category = "DrTable|Strings")
	TArray<FDrStringLanguage> Languages;

	const FDrStringLanguage* FindLanguage(const FString& Language) const;
	TArray<FString> GetLanguageNames() const;
};
