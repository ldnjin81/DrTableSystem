// Copyright ldnjin81. All Rights Reserved.

#include "DrLocalizedTextBlock.h"

#include "DrStringTables.h"

void UDrLocalizedTextBlock::SetStringKey(FName InTable, FName InKey)
{
	Table = InTable;
	Key = InKey;
	RefreshText();
}

void UDrLocalizedTextBlock::RefreshText()
{
	if (Table.IsNone() || Key.IsNone())
	{
		return;
	}
	// Without string tables (for example in the designer) the text set in the widget stays.
	if (const UDrStringTables* Tables = UDrStringTables::Get())
	{
		SetText(Tables->GetText(Table, Key));
	}
}

void UDrLocalizedTextBlock::SynchronizeProperties()
{
	Super::SynchronizeProperties();
	Bind();
	RefreshText();
}

void UDrLocalizedTextBlock::ReleaseSlateResources(bool bReleaseChildren)
{
	Unbind();
	Super::ReleaseSlateResources(bReleaseChildren);
}

void UDrLocalizedTextBlock::HandleLanguageChanged(const FString& Language)
{
	RefreshText();
}

void UDrLocalizedTextBlock::Bind()
{
	UDrStringTables* Tables = UDrStringTables::Get();
	if (Tables == BoundTables.Get() && LanguageChangedHandle.IsValid())
	{
		return;
	}
	Unbind();
	if (Tables)
	{
		LanguageChangedHandle = Tables->OnLanguageChanged.AddUObject(this, &UDrLocalizedTextBlock::HandleLanguageChanged);
		BoundTables = Tables;
	}
}

void UDrLocalizedTextBlock::Unbind()
{
	if (UDrStringTables* Tables = BoundTables.Get())
	{
		Tables->OnLanguageChanged.Remove(LanguageChangedHandle);
	}
	LanguageChangedHandle.Reset();
	BoundTables.Reset();
}
