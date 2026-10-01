// Copyright ldnjin81. All Rights Reserved.

#pragma once

#include "CoreMinimal.h"
#include "Subsystems/GameInstanceSubsystem.h"
#include "DrStringSubsystem.generated.h"

class UDrStringTables;
class FCulture;

DECLARE_DYNAMIC_MULTICAST_DELEGATE_OneParam(FDrOnStringLanguageChangedDynamic, const FString&, Language);

/**
 * String tables for the game: loads <AssetRoot>/Strings/DA_DrStrings, picks the starting
 * language and switches languages at runtime (only the current language stays in memory).
 *
 * Starting language: the language the player chose last time (GameUserSettings.ini, when
 * bRememberStringLanguage) -> the engine culture (ko-KR also matches ko) -> the manifest's
 * default (base) language. It loads synchronously so the first frame already has text.
 */
UCLASS()
class DRTABLERUNTIME_API UDrStringSubsystem : public UGameInstanceSubsystem
{
	GENERATED_BODY()

public:
	virtual void Initialize(FSubsystemCollectionBase& Collection) override;
	virtual void Deinitialize() override;

	/** Starts switching to Language (asynchronous). OnLanguageChanged fires when it is shown. */
	UFUNCTION(BlueprintCallable, Category = "DrTable|Strings")
	bool SetLanguage(const FString& Language);

	/** The language shown now. */
	UFUNCTION(BlueprintPure, Category = "DrTable|Strings")
	FString GetLanguage() const;

	/** The language being loaded, or empty. */
	UFUNCTION(BlueprintPure, Category = "DrTable|Strings")
	FString GetPendingLanguage() const;

	UFUNCTION(BlueprintPure, Category = "DrTable|Strings")
	TArray<FString> GetAvailableLanguages() const;

	UFUNCTION(BlueprintPure, Category = "DrTable|Strings")
	FText GetText(FName Table, FName Key) const;

	UFUNCTION(BlueprintPure, Category = "DrTable|Strings")
	bool TryGetText(FName Table, FName Key, FText& Text) const;

	/** Fires after a new language has loaded and replaced the previous one: refresh UI here. */
	UPROPERTY(BlueprintAssignable, Category = "DrTable|Strings")
	FDrOnStringLanguageChangedDynamic OnLanguageChanged;

	UDrStringTables* GetTables() const { return Tables; }

	/** The starting language for these languages (saved choice, then culture, then default). Public for tests. */
	static FString ChooseLanguage(const TArray<FString>& Available, const FString& Saved, const FString& Culture, const FString& Default);

private:
	void HandleLanguageChanged(const FString& Language);
	void HandleCultureChanged();

	UPROPERTY(Transient)
	TObjectPtr<UDrStringTables> Tables;

	FDelegateHandle CultureChangedHandle;
};
