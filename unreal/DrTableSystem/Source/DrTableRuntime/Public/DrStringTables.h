// Copyright ldnjin81. All Rights Reserved.

#pragma once

#include "CoreMinimal.h"
#include "Engine/StreamableManager.h"
#include "UObject/Object.h"
#include "DrStringTables.generated.h"

class UDrStringManifest;
class UDrStringTableAsset;
struct FDrStringLanguage;

DRTABLERUNTIME_API DECLARE_LOG_CATEGORY_EXTERN(LogDrStrings, Log, All);

/** Broadcast after a new language has fully loaded and replaced the previous one. */
DECLARE_MULTICAST_DELEGATE_OneParam(FDrOnStringLanguageChanged, const FString& /* Language */);

/**
 * The string tables of one language at a time.
 *
 * - SetLanguage loads the new language's assets asynchronously. The current language keeps
 *   showing until everything has loaded, then both are swapped at once, the previous
 *   language's assets are released (the garbage collector unloads them) and
 *   OnLanguageChanged is broadcast. Languages that are not current are not in memory.
 * - A request made while another language is still loading replaces it.
 * - UDrStringSubsystem owns the instance the game uses; tests may create their own.
 */
UCLASS()
class DRTABLERUNTIME_API UDrStringTables : public UObject
{
	GENERATED_BODY()

public:
	/** The active instance (the game instance's, or a test override); null when there is none. */
	static UDrStringTables* Get();
	static void SetOverride(UDrStringTables* Tables);
	static void ClearOverride();

	/** Text of Key in the current language through the active instance (generated Ref<StringTable> accessors call this). */
	static FText FindText(FName Table, FName Key);

	/** Makes this the instance Get() returns (when no override is set). */
	void MakeActive();
	void ClearActive();

	void SetManifest(UDrStringManifest* InManifest);
	UDrStringManifest* GetManifest() const { return Manifest; }

	/** Loads Language now (blocking) and switches to it. For the first language, before the first frame. */
	bool LoadLanguageSync(const FString& Language);

	/** Starts switching to Language. Returns false (and keeps the current language) when the manifest has no such language. */
	bool SetLanguage(const FString& Language);

	/** The language shown now (empty before the first load). */
	const FString& GetLanguage() const { return Current; }

	/** The language being loaded, or empty. */
	const FString& GetPendingLanguage() const { return Pending; }

	TArray<FString> GetAvailableLanguages() const;

	bool TryGetText(FName Table, FName Key, FText& OutText) const;

	/**
	 * Text of Key in the current language. A missing key logs a warning once and returns
	 * "<Table.Key>" in development builds and empty text in shipping builds.
	 */
	FText GetText(FName Table, FName Key) const;

	/** The string table assets of the current language that are held in memory. */
	const TArray<TObjectPtr<UDrStringTableAsset>>& GetLoadedAssets() const { return Loaded; }

	void DumpStatus(FOutputDevice& Out) const;

	FDrOnStringLanguageChanged OnLanguageChanged;

private:
	void OnAsyncLoaded(FString Language);
	bool CollectAssets(const FDrStringLanguage& Entry, TArray<UDrStringTableAsset*>& OutAssets) const;
	void Activate(const FString& Language, const TArray<UDrStringTableAsset*>& Assets);
	void CancelPending();

	UPROPERTY(Transient)
	TObjectPtr<UDrStringManifest> Manifest;

	/** Keeps the current language's assets alive; replacing it lets the previous ones unload. */
	UPROPERTY(Transient)
	TArray<TObjectPtr<UDrStringTableAsset>> Loaded;

	/** Table -> (asset, key -> index). FName keys compare case-insensitively; drtable rejects keys that differ only by case. */
	struct FLoadedTable
	{
		const UDrStringTableAsset* Asset = nullptr;
		TMap<FName, int32> Index;
	};
	TMap<FName, FLoadedTable> Tables;

	FString Current;
	FString Pending;
	TSharedPtr<FStreamableHandle> PendingHandle;
	FStreamableManager Streamable;
	mutable TSet<FString> WarnedMissing;
};
