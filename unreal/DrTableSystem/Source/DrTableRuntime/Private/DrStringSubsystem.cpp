// Copyright ldnjin81. All Rights Reserved.

#include "DrStringSubsystem.h"

#include "DrStringTableAsset.h"
#include "DrStringTables.h"
#include "DrTableSettings.h"
#include "Internationalization/Culture.h"
#include "Internationalization/Internationalization.h"
#include "Misc/ConfigCacheIni.h"

namespace DrStrings::Private
{
	const TCHAR* ConfigSection = TEXT("DrTable.Strings");
	const TCHAR* ConfigKey = TEXT("Language");
} // namespace DrStrings::Private

void UDrStringSubsystem::Initialize(FSubsystemCollectionBase& Collection)
{
	Super::Initialize(Collection);
	const UDrTableSettings* Settings = GetDefault<UDrTableSettings>();
	Tables = NewObject<UDrStringTables>(this);
	Tables->OnLanguageChanged.AddUObject(this, &UDrStringSubsystem::HandleLanguageChanged);
	Tables->MakeActive();

	const FString ManifestPath = FString::Printf(TEXT("%s/Strings/DA_DrStrings.DA_DrStrings"), *Settings->AssetRoot);
	UDrStringManifest* Manifest = TSoftObjectPtr<UDrStringManifest>(FSoftObjectPath(ManifestPath)).LoadSynchronous();
	if (!Manifest)
	{
		UE_LOG(LogDrStrings, Verbose, TEXT("[DrStrings] No string manifest at %s; no string tables."), *ManifestPath);
		return;
	}
	Tables->SetManifest(Manifest);

	FString Saved;
	if (Settings->bRememberStringLanguage && GConfig)
	{
		GConfig->GetString(DrStrings::Private::ConfigSection, DrStrings::Private::ConfigKey, Saved, GGameUserSettingsIni);
	}
	const FString Culture = FInternationalization::Get().GetCurrentLanguage()->GetName();
	const FString Language = ChooseLanguage(Manifest->GetLanguageNames(), Saved, Culture, Manifest->DefaultLanguage);
	if (!Language.IsEmpty())
	{
		Tables->LoadLanguageSync(Language);
	}
	if (Settings->bStringsFollowCulture)
	{
		CultureChangedHandle = FInternationalization::Get().OnCultureChanged().AddUObject(this, &UDrStringSubsystem::HandleCultureChanged);
	}
}

void UDrStringSubsystem::Deinitialize()
{
	if (CultureChangedHandle.IsValid())
	{
		FInternationalization::Get().OnCultureChanged().Remove(CultureChangedHandle);
		CultureChangedHandle.Reset();
	}
	if (Tables)
	{
		Tables->OnLanguageChanged.RemoveAll(this);
		Tables->ClearActive();
	}
	Super::Deinitialize();
}

FString UDrStringSubsystem::ChooseLanguage(const TArray<FString>& Available, const FString& Saved, const FString& Culture, const FString& Default)
{
	if (!Saved.IsEmpty() && Available.Contains(Saved))
	{
		return Saved;
	}
	// Exact culture first (zh-Hans), then its parents (ko-KR -> ko).
	FString Candidate = Culture;
	while (!Candidate.IsEmpty())
	{
		if (const FString* Found = Available.FindByPredicate([&Candidate](const FString& Language) { return Language.Equals(Candidate, ESearchCase::IgnoreCase); }))
		{
			return *Found;
		}
		int32 Dash = INDEX_NONE;
		if (!Candidate.FindLastChar(TEXT('-'), Dash))
		{
			break;
		}
		Candidate.LeftInline(Dash);
	}
	if (Available.Contains(Default))
	{
		return Default;
	}
	return Available.Num() > 0 ? Available[0] : FString();
}

bool UDrStringSubsystem::SetLanguage(const FString& Language)
{
	return Tables && Tables->SetLanguage(Language);
}

FString UDrStringSubsystem::GetLanguage() const
{
	return Tables ? Tables->GetLanguage() : FString();
}

FString UDrStringSubsystem::GetPendingLanguage() const
{
	return Tables ? Tables->GetPendingLanguage() : FString();
}

TArray<FString> UDrStringSubsystem::GetAvailableLanguages() const
{
	return Tables ? Tables->GetAvailableLanguages() : TArray<FString>();
}

FText UDrStringSubsystem::GetText(FName Table, FName Key) const
{
	return Tables ? Tables->GetText(Table, Key) : FText::GetEmpty();
}

bool UDrStringSubsystem::TryGetText(FName Table, FName Key, FText& Text) const
{
	return Tables && Tables->TryGetText(Table, Key, Text);
}

void UDrStringSubsystem::HandleLanguageChanged(const FString& Language)
{
	if (GetDefault<UDrTableSettings>()->bRememberStringLanguage && GConfig)
	{
		GConfig->SetString(DrStrings::Private::ConfigSection, DrStrings::Private::ConfigKey, *Language, GGameUserSettingsIni);
		GConfig->Flush(false, GGameUserSettingsIni);
	}
	OnLanguageChanged.Broadcast(Language);
}

void UDrStringSubsystem::HandleCultureChanged()
{
	if (!Tables)
	{
		return;
	}
	const FString Culture = FInternationalization::Get().GetCurrentLanguage()->GetName();
	const FString Language = ChooseLanguage(Tables->GetAvailableLanguages(), FString(), Culture, FString());
	if (!Language.IsEmpty() && Language != Tables->GetLanguage())
	{
		Tables->SetLanguage(Language);
	}
}
