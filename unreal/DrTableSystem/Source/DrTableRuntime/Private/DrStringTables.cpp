// Copyright ldnjin81. All Rights Reserved.

#include "DrStringTables.h"

#include "DrStringTableAsset.h"
#include "Misc/OutputDevice.h"

DEFINE_LOG_CATEGORY(LogDrStrings);

namespace DrStrings::Private
{
	bool bOverrideActive = false;
	TWeakObjectPtr<UDrStringTables> OverrideTables;
	TWeakObjectPtr<UDrStringTables> ActiveTables;
} // namespace DrStrings::Private

const FDrStringLanguage* UDrStringManifest::FindLanguage(const FString& Language) const
{
	return Languages.FindByPredicate([&Language](const FDrStringLanguage& Entry) { return Entry.Language == Language; });
}

TArray<FString> UDrStringManifest::GetLanguageNames() const
{
	TArray<FString> Names;
	for (const FDrStringLanguage& Entry : Languages)
	{
		Names.Add(Entry.Language);
	}
	return Names;
}

UDrStringTables* UDrStringTables::Get()
{
	if (DrStrings::Private::bOverrideActive)
	{
		return DrStrings::Private::OverrideTables.Get();
	}
	return DrStrings::Private::ActiveTables.Get();
}

void UDrStringTables::SetOverride(UDrStringTables* Tables)
{
	DrStrings::Private::bOverrideActive = true;
	DrStrings::Private::OverrideTables = Tables;
}

void UDrStringTables::ClearOverride()
{
	DrStrings::Private::bOverrideActive = false;
	DrStrings::Private::OverrideTables.Reset();
}

FText UDrStringTables::FindText(FName Table, FName Key)
{
	const UDrStringTables* Tables = Get();
	return Tables ? Tables->GetText(Table, Key) : FText::GetEmpty();
}

void UDrStringTables::MakeActive()
{
	DrStrings::Private::ActiveTables = this;
}

void UDrStringTables::ClearActive()
{
	if (DrStrings::Private::ActiveTables.Get() == this)
	{
		DrStrings::Private::ActiveTables.Reset();
	}
}

void UDrStringTables::SetManifest(UDrStringManifest* InManifest)
{
	CancelPending();
	Manifest = InManifest;
}

TArray<FString> UDrStringTables::GetAvailableLanguages() const
{
	return Manifest ? Manifest->GetLanguageNames() : TArray<FString>();
}

bool UDrStringTables::CollectAssets(const FDrStringLanguage& Entry, TArray<UDrStringTableAsset*>& OutAssets) const
{
	OutAssets.Reset();
	bool bComplete = true;
	for (const TSoftObjectPtr<UDrStringTableAsset>& Ref : Entry.Tables)
	{
		if (UDrStringTableAsset* Asset = Ref.Get())
		{
			OutAssets.Add(Asset);
		}
		else
		{
			bComplete = false;
		}
	}
	return bComplete;
}

bool UDrStringTables::LoadLanguageSync(const FString& Language)
{
	const FDrStringLanguage* Entry = Manifest ? Manifest->FindLanguage(Language) : nullptr;
	if (!Entry)
	{
		UE_LOG(LogDrStrings, Warning, TEXT("[DrStrings] Unknown language '%s'."), *Language);
		return false;
	}
	TArray<UDrStringTableAsset*> Assets;
	for (const TSoftObjectPtr<UDrStringTableAsset>& Ref : Entry->Tables)
	{
		UDrStringTableAsset* Asset = Ref.LoadSynchronous();
		if (!Asset)
		{
			UE_LOG(LogDrStrings, Warning, TEXT("[DrStrings] String table asset not found: %s (bake it with -run=DrTableBake)"), *Ref.ToString());
			return false;
		}
		Assets.Add(Asset);
	}
	CancelPending();
	Activate(Language, Assets);
	return true;
}

bool UDrStringTables::SetLanguage(const FString& Language)
{
	const FDrStringLanguage* Entry = Manifest ? Manifest->FindLanguage(Language) : nullptr;
	if (!Entry)
	{
		UE_LOG(LogDrStrings, Warning, TEXT("[DrStrings] Unknown language '%s'; keeping '%s'."), *Language, *Current);
		return false;
	}
	if (Language == Pending)
	{
		return true;
	}
	CancelPending();
	if (Language == Current)
	{
		return true;
	}

	// Already in memory (for example still referenced elsewhere): switch now.
	TArray<UDrStringTableAsset*> Assets;
	if (CollectAssets(*Entry, Assets))
	{
		Activate(Language, Assets);
		return true;
	}

	TArray<FSoftObjectPath> Paths;
	for (const TSoftObjectPtr<UDrStringTableAsset>& Ref : Entry->Tables)
	{
		Paths.Add(Ref.ToSoftObjectPath());
	}
	Pending = Language;
	TSharedPtr<FStreamableHandle> Handle = Streamable.RequestAsyncLoad(
		Paths, FStreamableDelegate::CreateWeakLambda(this, [this, Language]() { OnAsyncLoaded(Language); }));
	if (Pending == Language)
	{
		PendingHandle = Handle;
	}
	else if (Handle.IsValid())
	{
		// Completed during the request; the assets are held by Loaded now.
		Handle->ReleaseHandle();
	}
	return true;
}

void UDrStringTables::OnAsyncLoaded(FString Language)
{
	if (Pending != Language)
	{
		return;
	}
	const FDrStringLanguage* Entry = Manifest ? Manifest->FindLanguage(Language) : nullptr;
	TArray<UDrStringTableAsset*> Assets;
	if (!Entry || !CollectAssets(*Entry, Assets))
	{
		UE_LOG(LogDrStrings, Warning, TEXT("[DrStrings] Language '%s' did not load completely (bake it with -run=DrTableBake); keeping '%s'."), *Language, *Current);
		CancelPending();
		return;
	}
	TSharedPtr<FStreamableHandle> Handle = MoveTemp(PendingHandle);
	Pending.Reset();
	Activate(Language, Assets);
	if (Handle.IsValid())
	{
		Handle->ReleaseHandle();
	}
}

void UDrStringTables::CancelPending()
{
	if (PendingHandle.IsValid())
	{
		PendingHandle->CancelHandle();
		PendingHandle.Reset();
	}
	Pending.Reset();
}

void UDrStringTables::Activate(const FString& Language, const TArray<UDrStringTableAsset*>& Assets)
{
	TMap<FName, FLoadedTable> NewTables;
	for (const UDrStringTableAsset* Asset : Assets)
	{
		if (Asset->Keys.Num() != Asset->Values.Num())
		{
			UE_LOG(LogDrStrings, Warning, TEXT("[DrStrings] %s: keys (%d) and values (%d) differ in length; skipped."), *Asset->GetName(), Asset->Keys.Num(), Asset->Values.Num());
			continue;
		}
		FLoadedTable& Table = NewTables.Add(Asset->Table);
		Table.Asset = Asset;
		Table.Index.Reserve(Asset->Keys.Num());
		for (int32 Index = 0; Index < Asset->Keys.Num(); ++Index)
		{
			Table.Index.Add(Asset->Keys[Index], Index);
		}
	}
	// Replacing Loaded drops the previous language's assets; the garbage collector unloads them.
	Loaded.Reset();
	Loaded.Append(Assets);
	Tables = MoveTemp(NewTables);
	Current = Language;
	WarnedMissing.Reset();
	OnLanguageChanged.Broadcast(Current);
}

bool UDrStringTables::TryGetText(FName Table, FName Key, FText& OutText) const
{
	const FLoadedTable* Found = Tables.Find(Table);
	const int32* Index = Found ? Found->Index.Find(Key) : nullptr;
	if (!Index)
	{
		return false;
	}
	// Culture-invariant: this text is already in the chosen language and must not go through .locres.
	OutText = FText::AsCultureInvariant(Found->Asset->Values[*Index]);
	return true;
}

FText UDrStringTables::GetText(FName Table, FName Key) const
{
	FText Text;
	if (TryGetText(Table, Key, Text))
	{
		return Text;
	}
	if (Key.IsNone())
	{
		return FText::GetEmpty();
	}
	const FString Missing = FString::Printf(TEXT("%s.%s"), *Table.ToString(), *Key.ToString());
	if (!WarnedMissing.Contains(Missing))
	{
		WarnedMissing.Add(Missing);
		UE_LOG(LogDrStrings, Warning, TEXT("[DrStrings] No text for %s in '%s'."), *Missing, *Current);
	}
#if UE_BUILD_SHIPPING
	return FText::GetEmpty();
#else
	return FText::AsCultureInvariant(FString::Printf(TEXT("<%s>"), *Missing));
#endif
}

void UDrStringTables::DumpStatus(FOutputDevice& Out) const
{
	Out.Logf(TEXT("DrStrings: language '%s'%s, available: %s"), *Current,
		Pending.IsEmpty() ? TEXT("") : *FString::Printf(TEXT(" (loading '%s')"), *Pending),
		*FString::Join(GetAvailableLanguages(), TEXT(", ")));
	for (const TObjectPtr<UDrStringTableAsset>& Asset : Loaded)
	{
		Out.Logf(TEXT("  %s [%s]: %d key(s)"), *Asset->Table.ToString(), *Asset->Language, Asset->Keys.Num());
	}
}
