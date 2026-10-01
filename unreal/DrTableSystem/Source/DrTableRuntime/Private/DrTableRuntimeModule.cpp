// Copyright ldnjin81. All Rights Reserved.

#include "HAL/IConsoleManager.h"
#include "Modules/ModuleManager.h"
#include "DrStringTables.h"
#include "DrTableRegistry.h"
#include "DrTableSettings.h"

UDrTableSettings::UDrTableSettings()
	: AssetRoot(TEXT("/Game/Data"))
{
}

namespace DrTable::Private
{
	static FAutoConsoleCommandWithOutputDevice StatusCommand(
		TEXT("DrTable.Status"),
		TEXT("Lists the registered tables and how many rows each has loaded."),
		FConsoleCommandWithOutputDeviceDelegate::CreateLambda([](FOutputDevice& Out)
		{
			if (const UDrTableRegistry* Registry = UDrTableRegistry::Get())
			{
				Registry->DumpStatus(Out);
			}
			else
			{
				Out.Log(TEXT("DrTable: no active registry."));
			}
		}));

	static FAutoConsoleCommand ReloadCommand(
		TEXT("DrTable.Reload"),
		TEXT("Reloads every registered table from its baked asset. Row pointers obtained before the reload become invalid."),
		FConsoleCommandDelegate::CreateLambda([]()
		{
			if (UDrTableRegistry* Registry = UDrTableRegistry::Get())
			{
				Registry->RegisterAutoTables();
				Registry->ReloadAllTables();
			}
		}));

	static FAutoConsoleCommandWithOutputDevice StringsStatusCommand(
		TEXT("DrStrings.Status"),
		TEXT("Shows the current string language and the string tables loaded for it."),
		FConsoleCommandWithOutputDeviceDelegate::CreateLambda([](FOutputDevice& Out)
		{
			if (const UDrStringTables* Tables = UDrStringTables::Get())
			{
				Tables->DumpStatus(Out);
			}
			else
			{
				Out.Log(TEXT("DrStrings: no active string tables (they start with the game instance)."));
			}
		}));

	static FAutoConsoleCommand StringsLanguageCommand(
		TEXT("DrStrings.Language"),
		TEXT("Switches the string language: DrStrings.Language <culture>, e.g. DrStrings.Language en"),
		FConsoleCommandWithArgsDelegate::CreateLambda([](const TArray<FString>& Args)
		{
			if (UDrStringTables* Tables = UDrStringTables::Get(); Tables && Args.Num() == 1)
			{
				Tables->SetLanguage(Args[0]);
			}
		}));
} // namespace DrTable::Private

IMPLEMENT_MODULE(FDefaultModuleImpl, DrTableRuntime)
